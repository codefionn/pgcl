use log::debug;
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum LineMessage {
    Line(String, tokio::sync::oneshot::Sender<()>),
    Exit(),
}

pub struct CLIActor {
    tx: mpsc::Sender<LineMessage>,
}

impl CLIActor {
    pub fn new(tx: mpsc::Sender<LineMessage>) -> Self {
        Self { tx }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        debug!("Started {}", stringify!(CLIActor));

        let mut rl = Editor::<()>::new()?;
        rl.set_auto_add_history(true);

        loop {
            let readline = rl.readline("> ");

            match readline {
                Ok(line) => {
                    let (tx_confirm, rx_confirm) = tokio::sync::oneshot::channel();

                    self.tx.send(LineMessage::Line(line, tx_confirm)).await?;
                    self.tx.reserve().await?;
                    rx_confirm.await?;
                }
                Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                    self.tx.send(LineMessage::Exit()).await.ok();
                    self.tx.reserve().await.ok();

                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);

                    self.tx.send(LineMessage::Exit()).await.ok();
                    self.tx.reserve().await.ok();

                    break;
                }
            };
        }

        Ok(())
    }
}
