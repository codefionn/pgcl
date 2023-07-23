use log::{debug, error};
use tokio::{sync::mpsc, sync::oneshot, task::JoinHandle};

use crate::{
    context::ContextHandler,
    execute::{Executor, Syntax},
    system::SystemHandler,
};

pub enum Message {
    Signal(Syntax),
    Exit(oneshot::Sender<()>),
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signal(expr) => f.write_str(format!("Signal({}", expr).as_str()),
            Self::Exit(_)  => f.write_str("Exit()")
        }
    }
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
        let mut executor = Executor::new(&mut ctx, &mut system);
        let mut init = init.clone();

        let mut exit_handlers: Vec<oneshot::Sender<()>> = Vec::new();

        while let Some(msg) = rx.recv().await {
            debug!("Actor received message: {:?}", msg);
            match msg {
                Message::Signal(expr) => {
                    let expr = executor
                        .execute(
                            Syntax::Call(
                                Box::new(Syntax::Call(
                                    Box::new(actor_fn.clone()),
                                    Box::new(init.clone()),
                                )),
                                Box::new(expr),
                            ),
                            false,
                        )
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
                Message::Exit(exit_handle) => {
                    exit_handlers.push(exit_handle);

                    break;
                }
            }


            debug!("Actor waiting for next message");
        }

        debug!("Actor quitting due to receiving exit signal");

        for exit_handler in exit_handlers {
            exit_handler.send(());
        }
    });

    (handle, tx)
}
