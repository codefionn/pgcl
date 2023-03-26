use log::debug;
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineMessage {
    Line(String),
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

            let msg = match readline {
                Ok(line) => LineMessage::Line(line),
                Err(ReadlineError::Interrupted) => LineMessage::Exit(),
                Err(ReadlineError::Eof) => LineMessage::Exit(),
                Err(err) => {
                    println!("Error: {:?}", err);
                    LineMessage::Exit()
                }
            };

            let is_exit = LineMessage::Exit() == msg;
            self.tx.send(msg).await?;
            self.tx.reserve().await?;

            if is_exit {
                break;
            }
        }

        Ok(())
    }
}
