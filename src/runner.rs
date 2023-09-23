use tokio::sync::{mpsc, oneshot};

use crate::{context::ContextHandler, execute::Syntax, gc::mark_used, system::SystemHandler};

pub enum RunnerMessage {
    Mark(
        /* gc_even: */ bool,
        /* result: */ oneshot::Sender<()>,
    ),
    Sweep(/* result: */ oneshot::Sender<()>),
}

pub struct Runner {
    id: usize,
    rx: mpsc::Receiver<RunnerMessage>,
    system: SystemHandler,
}

impl Runner {
    pub async fn new(system: &mut SystemHandler) -> anyhow::Result<Runner> {
        let (tx, rx) = mpsc::channel(1);
        let id = system.create_runner(tx).await?;

        Ok(Runner {
            id,
            rx,
            system: system.clone(),
        })
    }

    pub async fn handle(
        &mut self,
        ctx: Option<&mut ContextHandler>,
        syntax_exprs: &[&Syntax],
    ) -> anyhow::Result<()> {
        if let Ok(msg) = self.rx.try_recv() {
            match msg {
                RunnerMessage::Mark(gc_even, result) => {
                    mark_used(&mut self.system, syntax_exprs).await;
                    if let Some(ctx) = ctx {
                        ctx.mark(&mut self.system, gc_even).await;
                    }
                    let _ = result.send(());
                }
                RunnerMessage::Sweep(result) => {
                    let _ = result.send(());
                }
            }
        }

        Ok(())
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        let mut system = self.system.clone();
        let id = self.id;
        tokio::spawn(async move {
            system.drop_runner(id).await;
        });
    }
}
