use log::{debug, error};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{context::ContextHandler, execute::Syntax, system::SystemHandler};

#[derive(Debug, Clone)]
pub enum Message {
    Signal(Syntax),
    Exit(),
}

pub async fn create_actor(
    ctx: ContextHandler,
    system: SystemHandler,
    init: Syntax,
    actor_fn: Syntax,
) -> (JoinHandle<()>, mpsc::Sender<Message>) {
    let (tx, mut rx) = mpsc::channel(256);

    let handle = tokio::spawn(async move {
        let mut ctx = ctx.clone();
        let mut system = system.clone();
        let mut init = init.clone();

        while let Some(msg) = rx.recv().await {
            debug!("Actor received message: {:?}", msg);
            match msg {
                Message::Signal(expr) => {
                    let expr = Syntax::Call(
                        Box::new(Syntax::Call(
                            Box::new(actor_fn.clone()),
                            Box::new(init.clone()),
                        )),
                        Box::new(expr),
                    )
                    .execute(false, &mut ctx, &mut system)
                    .await;

                    match expr {
                        Ok(expr) => {
                            init = expr;
                            debug!("Successfully executed actor");
                        }
                        Err(err) => {
                            error!("Actor failed: {:?}", err);
                        }
                    }
                }
                Message::Exit() => {
                    debug!("Actor quitting due to receiving exit signal");
                    break;
                }
            }
        }
    });

    (handle, tx)
}
