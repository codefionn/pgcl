use log::debug;
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum LineMessage {
    Line(String, tokio::sync::oneshot::Sender<ExecutedMessage>),
    Exit(),
}

#[derive(Debug, PartialEq)]
pub enum ExecutedMessage {
    Continue(),
    Exit(i32),
}

pub struct CLIActor {
    tx: mpsc::Sender<LineMessage>,
}

impl CLIActor {
    pub fn new(tx: mpsc::Sender<LineMessage>) -> Self {
        Self { tx }
    }

    #[must_use]
    pub async fn run(self) -> anyhow::Result<i32> {
        debug!("Started {}", stringify!(CLIActor));

        let mut rl = Editor::<()>::new()?;
        rl.set_auto_add_history(true);

        let mut exit_code = 0;
        loop {
            let readline = rl.readline("> "); // Print '> ' on an interactive console

            match readline {
                Ok(line) => {
                    let (tx_confirm, rx_confirm) = tokio::sync::oneshot::channel();

                    self.tx.send(LineMessage::Line(line, tx_confirm)).await?;
                    self.tx.reserve().await?; // Flush the channel

                    match rx_confirm.await? {
                        ExecutedMessage::Continue() => {}
                        ExecutedMessage::Exit(code) => {
                            exit_code = code;

                            // Ignore the results, because the channel stops working afterwards
                            self.tx.send(LineMessage::Exit()).await.ok();
                            self.tx.reserve().await.ok(); // Flush the channel

                            break;
                        }
                    }; // Await the completion of execution process
                }
                Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                    // Ignore the results, because the channel stops working afterwards
                    self.tx.send(LineMessage::Exit()).await.ok();
                    self.tx.reserve().await.ok(); // Flush the channel

                    break;
                }
                Err(err) => {
                    exit_code = 1;

                    eprintln!("Error: {err:?}");

                    // Ignore the results, because the channel stops working afterwards
                    self.tx.send(LineMessage::Exit()).await.ok();
                    self.tx.reserve().await.ok(); // Flush the channel

                    break;
                }
            };
        }

        Ok(exit_code)
    }
}
