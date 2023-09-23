use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use log::{debug, error};
use tokio::{
    sync::mpsc,
    sync::{oneshot, RwLock},
    task::JoinHandle,
};

use crate::{
    context::ContextHandler,
    execute::{Executor, Syntax},
    runner::Runner,
    system::SystemHandler,
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

struct Actor {
    ctx: ContextHandler,
    system: SystemHandler,
    init: Syntax,
    actor_fn: Syntax,
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
    running: Arc<AtomicBool>,
}

impl Actor {
    async fn run_actor(mut self) {
        let mut runner = Runner::new(&mut self.system).await.unwrap();
        let executor = RwLock::new(Executor::new(
            &mut self.ctx,
            &mut self.system,
            &mut runner,
            false,
        ));
        let mut init = self.init.clone();

        let mut exit_handlers: Vec<oneshot::Sender<()>> = Vec::new();

        while let Some(msg) = self.rx.recv().await {
            #[cfg(debug_assertions)]
            debug!("Actor received message: {:?}", msg);

            match msg {
                Message::Signal(expr) => {
                    self.running.store(true, Ordering::Relaxed);

                    let expr = executor
                        .write()
                        .await
                        .execute(
                            Syntax::Call(
                                Box::new(Syntax::Call(
                                    Box::new(self.actor_fn.clone()),
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
                            //#[cfg(debug_assertions)]
                            //debug!("Successfully executed actor");
                        }
                        Err(err) => {
                            error!("Actor failed: {:?}", err);
                        }
                    }

                    self.running.store(false, Ordering::Relaxed);
                }
                Message::Wakeup() => {
                    let mut executor = executor.write().await;
                    if let Err(err) = executor.runner_handle(&[&init, &self.actor_fn]).await {
                        log::error!("{:?}", err);
                    }
                }
                Message::Exit(exit_handle) => {
                    exit_handlers.push(exit_handle);

                    break;
                }
            }

            //#[cfg(debug_assertions)]
            //debug!("Actor waiting for next message");
        }

        //#[cfg(debug_assertions)]
        //debug!("Actor quitting due to receiving exit signal");

        for exit_handler in exit_handlers {
            if let Err(err) = exit_handler.send(()) {
                log::error!("{:?}", err);
            }
        }
    }
}

pub async fn create_actor(
    ctx: ContextHandler,
    system: SystemHandler,
    init: Syntax,
    actor_fn: Syntax,
) -> (JoinHandle<()>, mpsc::Sender<Message>, Arc<AtomicBool>) {
    let (tx, rx) = mpsc::channel(256);
    let running = Arc::new(AtomicBool::new(false));

    let handle = tokio::spawn({
        let running = running.clone();
        let tx = tx.clone();

        let actor = Actor {
            ctx,
            system,
            init,
            actor_fn,
            tx,
            rx,
            running,
        };

        async move {
            actor.run_actor().await;
        }
    });

    (handle, tx, running)
}

pub struct ActorHandle {
    pub tx: mpsc::Sender<crate::actor::Message>,
    pub handle: JoinHandle<()>,
    pub used: bool,
    pub running: Arc<AtomicBool>,
}

impl ActorHandle {
    pub async fn destroy(self) -> JoinHandle<()> {
        let (tx, rx) = oneshot::channel();
        if let Ok(()) = self.tx.send(crate::actor::Message::Exit(tx)).await {
            let _ = rx.await;
        }

        self.handle
    }
}
