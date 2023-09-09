use log::{debug, warn};
use rowan::GreenNodeBuilder;
use tokio::sync::{mpsc, oneshot};

use crate::{
    context::{ContextHandler, ContextHolder},
    errors::InterpreterError,
    execute::Executor,
    execute::{BiOpType, Syntax},
    lexer::Token,
    parser::{print_ast, Parser, SyntaxKind},
    reader::{ExecutedMessage, LineMessage},
    runner::Runner,
    system::SystemHandler,
    Args,
};

/// Actor for interpreting input lines from the CLI
pub struct InterpreterActor {
    args: Args,
    rx: mpsc::Receiver<LineMessage>,
}

impl InterpreterActor {
    pub fn new(args: Args, rx: mpsc::Receiver<LineMessage>) -> Self {
        Self { args, rx }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        //#[cfg(debug_assertions)]
        //debug!("Started {}", stringify!(InterpreterActor));

        let (tx_lexer, rx_lexer) = mpsc::channel(1);

        let (h0, h1) = tokio::join!(
            tokio::spawn({
                let tx_lexer = tx_lexer.clone();
                async move {
                    let lexer = InterpreterLexerActor::new(self.rx, tx_lexer);
                    lexer.run().await
                }
            }),
            tokio::spawn({
                let _args = self.args.clone();

                async move {
                    let execute = InterpreterExecuteActor::new(self.args, tx_lexer, rx_lexer);
                    execute.run().await
                }
            })
        );

        h0??;
        h1??;

        Ok(())
    }
}

/// Actor for lexing a line
struct InterpreterLexerActor {
    rx: mpsc::Receiver<LineMessage>,
    tx: mpsc::Sender<LexerMessage>,
}

impl InterpreterLexerActor {
    fn new(rx: mpsc::Receiver<LineMessage>, tx: mpsc::Sender<LexerMessage>) -> Self {
        Self { rx, tx }
    }

    async fn run(mut self) -> anyhow::Result<()> {
        //#[cfg(debug_assertions)]
        //debug!("Started {}", stringify!(InterpreterLexerActor));

        while let Some(msg) = self.rx.recv().await {
            //#[cfg(debug_assertions)]
            //debug!("Interpreter: {:?}", msg);

            match msg {
                LineMessage::Line(line, tx_confirm) => {
                    if line.trim().is_empty() {
                        tx_confirm
                            .send(ExecutedMessage::Continue())
                            .map_err(|err| anyhow::anyhow!("Actor-Line: {:?}", err))?;

                        continue;
                    }

                    self.tx
                        .send(LexerMessage::Line(
                            Token::lex_for_rowan(line.as_str()),
                            tx_confirm,
                        ))
                        .await?;
                    self.tx.reserve().await?;
                }
                _ => {
                    self.tx.send(LexerMessage::Exit()).await.ok();
                    self.tx.reserve().await.ok();

                    break;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum LexerMessage {
    Line(
        Vec<(Token, String)>,
        tokio::sync::oneshot::Sender<ExecutedMessage>,
    ),
    Wakeup(),
    Exit(),
    RealExit(),
}

/// Actor for executing results from the lexer
struct InterpreterExecuteActor {
    args: Args,
    rx: mpsc::Receiver<LexerMessage>,
    tx: mpsc::Sender<LexerMessage>,
    last_result: Option<Syntax>,
    ctx: ContextHolder,
    system: SystemHandler,
}

impl InterpreterExecuteActor {
    fn new(args: Args, tx: mpsc::Sender<LexerMessage>, rx: mpsc::Receiver<LexerMessage>) -> Self {
        Self {
            args,
            rx,
            tx,
            last_result: None,
            ctx: ContextHolder::default(),
            system: SystemHandler::default(),
        }
    }

    async fn run(mut self) -> anyhow::Result<()> {
        let mut exit_handle = None;

        //#[cfg(debug_assertions)]
        //debug!("Started {}", stringify!(InterpreterExecuteActor));

        let path = std::env::current_dir()?;

        let mut main_ctx: ContextHandler = ContextHandler::new(
            self.ctx
                .create_context("<stdin>".to_string(), Some(path))
                .await
                .get_id(),
            self.ctx.clone(),
        );

        let mut main_system: SystemHandler = self.system.get_handler(0).await.unwrap();
        main_system.set_lexer(self.tx).await;
        let mut runner = Runner::new(&mut main_system).await?;
        let mut executor = Executor::new(
            &mut main_ctx,
            &mut main_system,
            &mut runner,
            self.args.verbose,
        );

        // Save the length of the error vec in the main context
        // This helps to only output current errors
        let mut last_error_len = 0;

        // This saves tokens from a previous input line, that could not be parsed successfully.
        // This is for trying to combine these with the new input line, if the user completed the
        // statement from the previous line(s).
        let mut leftover_tokens: Vec<Vec<(SyntaxKind, String)>> = Vec::new();

        while let Some(msg) = self.rx.recv().await {
            //#[cfg(debug_assertions)]
            //debug!("Interpreter: {:?}", msg);

            match msg {
                LexerMessage::Line(lexer_result, tx_confirm) => {
                    let toks: Vec<(SyntaxKind, String)> = lexer_result
                        .into_iter()
                        .map(
                            |(tok, slice)| -> Result<(SyntaxKind, String), InterpreterError> {
                                Ok((tok.try_into()?, slice))
                            },
                        )
                        .try_collect()
                        .map_err(|err| anyhow::anyhow!("Interpreter-Line: {err:?}"))?;

                    let mut success = false;
                    let mut exit_code = None;

                    leftover_tokens.push(toks); // Push directly to the leftover tokens, this
                                                // simplifies the algorithm
                    for i in 0..leftover_tokens.len() {
                        let typed = {
                            // Combine the leftover tokens from i till the end
                            // (the end is the latest input line)
                            let prepend = leftover_tokens[i..]
                                .iter()
                                .flat_map(|toks| toks.clone().into_iter());

                            parse_to_typed(
                                &self.args,
                                prepend.collect(),
                                i != leftover_tokens.len() - 1,
                            )
                        };

                        //#[cfg(debug_assertions)]
                        //debug!("{:?}", typed);
                        if let Ok(typed) = typed {
                            success = true;

                            let typed = match typed {
                                Syntax::Pipe(expr) if self.last_result.is_some() => Syntax::BiOp(
                                    BiOpType::OpPipe,
                                    Box::new(self.last_result.as_ref().unwrap().clone()),
                                    expr,
                                ),
                                _ => typed,
                            };

                            let reduced: Syntax = typed.reduce().await;

                            //#[cfg(debug_assertions)]
                            //debug!("{}", reduced);
                            match executor.execute(reduced, true).await {
                                Ok(executed) => {
                                    if executed != Syntax::ValAny() {
                                        println!("{executed}");
                                    }

                                    self.last_result = Some(executed);
                                }
                                Err(InterpreterError::ProgramTerminatedByUser(id)) => {
                                    exit_code = Some(id);
                                }
                                Err(_) => {}
                            }

                            let errors = executor.get_ctx().get_errors().await;
                            if last_error_len < errors.len() {
                                eprintln!("{:?}", &errors[last_error_len..]);
                                last_error_len = errors.len();
                            }

                            break; // Success! (at least at parsing)
                        } else {
                            warn!("{:?}", typed);
                        }
                    }

                    if success {
                        // The parsing was done successfully => delete all leftover tokens, because
                        // they are not required anymore/the previous ones may really be errors.
                        leftover_tokens.clear();
                    }

                    // Confirm, that the parser has stopped => request a new input line
                    tx_confirm
                        .send(match exit_code {
                            None => ExecutedMessage::Continue(),
                            Some(id) => ExecutedMessage::Exit(id),
                        })
                        .map_err(|err| anyhow::anyhow!("Interpreter-Line: {err:?}"))?;
                }
                LexerMessage::Wakeup() => {
                    if let Err(err) = executor.runner_handle(&[]).await {
                        log::error!("{:?}", err);
                    }
                }
                LexerMessage::Exit() => {
                    exit_handle = Some(tokio::spawn({
                        let mut system = self.system.clone();
                        async move { system.exit().await }
                    }));
                }
                LexerMessage::RealExit() => {
                    break;
                }
            }
        }

        if let Some(exit_handle) = exit_handle {
            if let Err(err) = exit_handle.await {
                log::error!("{}", err);
            }
        }

        Ok(())
    }
}

fn parse_to_typed(
    args: &Args,
    toks: Vec<(SyntaxKind, String)>,
    ignore_errors: bool,
) -> Result<Syntax, InterpreterError> {
    let (ast, errors) =
        Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse_main(true);
    if !errors.is_empty() && !ignore_errors {
        eprintln!("{errors:?}");
    }

    if args.verbose {
        print_ast(0, &ast);
    }

    (*ast).try_into()
}
