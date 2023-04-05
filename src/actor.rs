use log::error;
use tokio::sync::mpsc;

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
) -> mpsc::Sender<Message> {
    let (tx, mut rx) = mpsc::channel(1);

    tokio::spawn(async move {
        let mut ctx = ctx.clone();
        let mut system = system.clone();
        let mut init = init.clone();

        while let Some(msg) = rx.recv().await {
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

                    if let Ok(expr) = expr {
                        init = expr;
                    } else {
                        error!("Actor failed");
                        break;
                    }
                }
                Message::Exit() => {
                    break;
                }
            }
        }
    });

    return tx;
}
