use log::debug;
use rowan::GreenNodeBuilder;
use tokio::sync::mpsc;

use crate::{
    errors::InterpreterError,
    execute::{BiOpType, Context, Syntax},
    lexer::Token,
    parser::{print_ast, Parser, SyntaxElement, SyntaxKind},
    reader::LineMessage,
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

    pub async fn run(mut self) -> anyhow::Result<()> {
        debug!("Started {}", stringify!(InterpreterActor));

        let (tx_lexer, rx_lexer) = mpsc::channel(1);
        let lexer = InterpreterLexerActor::new(self.rx, tx_lexer);
        let execute = InterpreterExecuteActor::new(self.args, rx_lexer);

        let (h0, h1) = tokio::join!(tokio::spawn(lexer.run()), tokio::spawn(execute.run()));

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
                LineMessage::Line(line) => {
                    self.tx
                        .send(LexerMessage::Line(Token::lex_for_rowan(line.as_str())))
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

#[derive(Clone, Debug)]
pub enum LexerMessage {
    Line(Vec<(Token, String)>),
    Exit(),
}

/// Actor for executing results from the lexer
struct InterpreterExecuteActor {
    args: Args,
    rx: mpsc::Receiver<LexerMessage>,
    last_result: Option<Syntax>,
    ctx: Context,
}

impl InterpreterExecuteActor {
    fn new(args: Args, rx: mpsc::Receiver<LexerMessage>) -> Self {
        Self {
            args,
            rx,
            last_result: None,
            ctx: Context::default(),
        }
    }

    async fn run(mut self) -> anyhow::Result<()> {
        debug!("Started {}", stringify!(InterpreterExecuteActor));

        while let Some(msg) = self.rx.recv().await {
            match msg {
                LexerMessage::Line(lexer_result) => {
                    debug!("{:?}", lexer_result);

                    let toks: Vec<(SyntaxKind, String)> = lexer_result
                        .into_iter()
                        .map(
                            |(tok, slice)| -> Result<(SyntaxKind, String), InterpreterError> {
                                Ok((tok.clone().try_into()?, slice.clone()))
                            },
                        )
                        .try_collect()
                        .map_err(|err| anyhow::anyhow!("{:?}", err))?;

                    let (ast, errors) =
                        Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
                    let ast: SyntaxElement = ast.into();
                    if !errors.is_empty() {
                        println!("{:?}", errors);
                    }

                    if self.args.verbose {
                        print_ast(0, ast.clone());
                    }

                    let typed: Result<Syntax, InterpreterError> = ast.try_into();
                    debug!("{:?}", typed);
                    if let Ok(typed) = typed {
                        let typed = match typed {
                            Syntax::Pipe(expr) if self.last_result.is_some() => Syntax::BiOp(
                                BiOpType::OpPipe,
                                Box::new(self.last_result.as_ref().unwrap().clone()),
                                expr,
                            ),
                            _ => typed,
                        };

                        let reduced = typed.reduce();
                        debug!("{}", reduced);
                        if let Ok(executed) = reduced.execute(true, &mut self.ctx) {
                            println!("{}", executed);
                            self.last_result = Some(executed);
                        }
                    }
                }
                _ => break,
            }
        }

        Ok(())
    }
}
