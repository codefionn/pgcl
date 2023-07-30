use log::{debug, error};
use tokio::{sync::mpsc, sync::{oneshot, RwLock}, task::JoinHandle};

use crate::{
    context::ContextHandler,
    execute::{Executor, Syntax},
    system::SystemHandler, runner::Runner,
};

pub enum Message {
    Signal(Syntax),
    Wakeup(),
    Exit(oneshot::Sender<()>),
}

impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signal(expr) => f.write_str(format!("Signal({})", expr).as_str()),
            Self::Wakeup() => f.write_str("Wakeup()"),
            Self::Exit(_) => f.write_str("Exit()"),
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
        let mut runner = Runner::new(&mut system).await.unwrap();
        let mut executor = RwLock::new(Executor::new(&mut ctx, &mut system, &mut runner, false));
        let mut init = init.clone();

        let mut exit_handlers: Vec<oneshot::Sender<()>> = Vec::new();

        while let Some(msg) = rx.recv().await {
            #[cfg(debug_assertions)]
            debug!("Actor received message: {:?}", msg);

            match msg {
                Message::Signal(expr) => {
                    let expr = executor.write().await
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
                            #[cfg(debug_assertions)]
                            debug!("Successfully executed actor");
                        }
                        Err(err) => {
                            error!("Actor failed: {:?}", err);
                        }
                    }
                }
                Message::Wakeup() => {
                    let mut executor = executor.write().await;
                    executor.runner_handle(&[&init]).await;
                }
                Message::Exit(exit_handle) => {
                    exit_handlers.push(exit_handle);

                    break;
                }
            }

            #[cfg(debug_assertions)]
            debug!("Actor waiting for next message");
        }

        #[cfg(debug_assertions)]
        debug!("Actor quitting due to receiving exit signal");

        for exit_handler in exit_handlers {
            exit_handler.send(());
        }
    });

    (handle, tx)
}

pub struct ActorHandle {
    pub tx: mpsc::Sender<crate::actor::Message>,
    pub handle: JoinHandle<()>,
    pub used: bool,
}

impl ActorHandle {
    pub async fn destroy(self) -> JoinHandle<()> {
        let (tx, rx) = oneshot::channel();
        if let Ok(()) = self.tx.send(crate::actor::Message::Exit(tx)).await {
            rx.await;
        }

        self.handle
    }
}
