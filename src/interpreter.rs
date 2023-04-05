use log::debug;
use rowan::GreenNodeBuilder;
use tokio::sync::mpsc;

use crate::{
    context::{ContextHandler, ContextHolder},
    errors::InterpreterError,
    execute::{BiOpType, Syntax},
    lexer::Token,
    parser::{print_ast, Parser, SyntaxKind},
    reader::LineMessage,
    system::{SystemHandler, SystemHolder},
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
        debug!("Started {}", stringify!(InterpreterActor));

        let (tx_lexer, rx_lexer) = mpsc::channel(1);

        let (h0, h1) = tokio::join!(
            tokio::spawn(async move {
                let lexer = InterpreterLexerActor::new(self.rx, tx_lexer);
                lexer.run().await
            }),
            tokio::spawn({
                let _args = self.args.clone();

                async move {
                    let execute = InterpreterExecuteActor::new(self.args, rx_lexer);
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
        debug!("Started {}", stringify!(InterpreterLexerActor));

        while let Some(msg) = self.rx.recv().await {
            match msg {
                LineMessage::Line(line, tx_confirm) => {
                    if line.trim().is_empty() {
                        tx_confirm
                            .send(())
                            .map_err(|err| anyhow::anyhow!("{:?}", err))?;

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
    Line(Vec<(Token, String)>, tokio::sync::oneshot::Sender<()>),
    Exit(),
}

/// Actor for executing results from the lexer
struct InterpreterExecuteActor {
    args: Args,
    rx: mpsc::Receiver<LexerMessage>,
    last_result: Option<Syntax>,
    ctx: ContextHolder,
    system: SystemHolder,
}

impl InterpreterExecuteActor {
    fn new(args: Args, rx: mpsc::Receiver<LexerMessage>) -> Self {
        Self {
            args,
            rx,
            last_result: None,
            ctx: ContextHolder::default(),
            system: SystemHolder::default(),
        }
    }

    async fn run(mut self) -> anyhow::Result<()> {
        debug!("Started {}", stringify!(InterpreterExecuteActor));

        let path = std::env::current_dir()?;

        let mut main_ctx: ContextHandler = ContextHandler::new(
            self.ctx
                .create_context("<stdin>".to_string(), Some(path))
                .await
                .get_id(),
            self.ctx.clone(),
        );

        let mut main_system: SystemHandler = self.system.get_handler(0).await.unwrap();

        let mut last_error_len = 0;

        let mut leftover_tokens: Vec<Vec<(SyntaxKind, String)>> = Vec::new();

        while let Some(msg) = self.rx.recv().await {
            match msg {
                LexerMessage::Line(lexer_result, tx_confirm) => {
                    let toks: Vec<(SyntaxKind, String)> = lexer_result
                        .into_iter()
                        .map(
                            |(tok, slice)| -> Result<(SyntaxKind, String), InterpreterError> {
                                Ok((tok.clone().try_into()?, slice.clone()))
                            },
                        )
                        .try_collect()
                        .map_err(|err| anyhow::anyhow!("{:?}", err))?;

                    let mut success = false;
                    leftover_tokens.push(toks);
                    for i in 0..leftover_tokens.len() {
                        let typed = {
                            let prepend = leftover_tokens[i..]
                                .into_iter()
                                .map(|toks| toks.clone().into_iter())
                                .flatten();

                            parse_to_typed(
                                &self.args,
                                prepend.collect(),
                                i != leftover_tokens.len() - 1,
                            )
                        };

                        debug!("{:?}", typed);
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

                            debug!("{}", reduced);
                            if let Ok(executed) =
                                reduced.execute(true, &mut main_ctx, &mut main_system).await
                            {
                                if executed != Syntax::ValAny() {
                                    println!("{}", executed);
                                }

                                self.last_result = Some(executed);
                            }

                            let errors = main_ctx.get_errors().await;
                            if last_error_len < errors.len() {
                                eprintln!("{:?}", &errors[last_error_len..]);
                                last_error_len = errors.len();
                            }

                            break;
                        }
                    }

                    if success {
                        leftover_tokens.clear();
                    }

                    tx_confirm
                        .send(())
                        .map_err(|err| anyhow::anyhow!("{:?}", err))?;
                }
                _ => {
                    self.system.drop_actors().await;

                    break;
                }
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
        eprintln!("{:?}", errors);
    }

    if args.verbose {
        print_ast(0, &ast);
    }

    return (*ast).try_into();
}
