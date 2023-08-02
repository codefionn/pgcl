use std::collections::{VecDeque, HashSet};

use tokio::sync::{oneshot, mpsc};

use crate::{system::SystemHandler, execute::Syntax, context::{Context, ContextHandler}};

pub enum RunnerMessage {
    Mark(/* result: */ oneshot::Sender<()>),
    Sweep(/* result: */ oneshot::Sender<()>)
}

pub struct Runner {
    id: usize,
    rx: mpsc::Receiver<RunnerMessage>,
    system: SystemHandler
}

impl Runner {
    pub async fn new(system: &mut SystemHandler) -> anyhow::Result<Runner> {
        let (tx, rx) = mpsc::channel(1);
        let id = system.create_runner(tx).await?;

        Ok(Runner {
            id,
            rx,
            system: system.clone()
        })
    }

    pub async fn handle(&mut self, ctx: Option<&mut ContextHandler>, syntax_exprs: &[&Syntax]) -> anyhow::Result<()> {
        let msg = self.rx.try_recv()?;
        match msg {
            RunnerMessage::Mark(result) => {
                for syntax in syntax_exprs {
                    mark_used(&mut self.system, syntax).await;
                }
                if let Some(ctx) = ctx {
                    for syntax in ctx.get_globals().await {
                        mark_used(&mut self.system, &syntax).await;
                    }
                    for syntax in ctx.get_values().await {
                        mark_used(&mut self.system, &syntax).await;
                    }
                }
                result.send(());
            }
            RunnerMessage::Sweep(result) => {
                result.send(());
            }
        }

        Ok(())
    }
}

async fn mark_used(system: &mut SystemHandler, syntax: &Syntax) {
    let mut broadsearch: VecDeque<&Syntax> = VecDeque::new();
    broadsearch.push_front(syntax);
    let mut already_marked = HashSet::new();

    while let Some(syntax) = broadsearch.pop_front() {
        match syntax {
            Syntax::Signal(signal_type, id) => {
                // Nur wenn hier noch nicht markiert erneut einfügen
                if already_marked.insert((signal_type, id)) {
                    system.mark_use_signal(signal_type.clone(), *id).await;
                }
            }
            _ => {
                broadsearch.extend(syntax.exprs().into_iter())
            }
        }
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
