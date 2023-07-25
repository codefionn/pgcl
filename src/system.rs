use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use anyhow::anyhow;
use futures::{future::join_all, SinkExt};
use log::{debug, error};
use tokio::{
    sync::{broadcast, mpsc, oneshot, Mutex},
    task::JoinHandle,
};

pub use crate::syscall::{PrivateSystem, SystemCallType};
use crate::{
    actor,
    context::ContextHandler,
    errors::InterpreterError,
    execute::{SignalType, Syntax},
};

#[derive(Clone)]
pub struct System {
    id: usize,
    private_system: Arc<Mutex<PrivateSystem>>,
}

impl System {
    pub fn get_id(&self) -> usize {
        self.id
    }

    pub async fn do_syscall(
        &self,
        ctx: &mut ContextHandler,
        system: &mut SystemHandler,
        no_change: bool,
        syscall: SystemCallType,
        expr: Syntax,
    ) -> Result<Syntax, InterpreterError> {
        self.private_system
            .lock()
            .await
            .do_syscall(ctx, system, no_change, syscall, expr)
            .await
    }

    pub async fn get(&self, syscall: SystemCallType) -> Option<Syntax> {
        self.private_system.lock().await.get(syscall).await
    }
}

pub enum SystemHandleMessage {}

enum SystemActorMessage {
    MarkUseSignal(SignalType, usize),
    StartCollectGarbage(),
    StopCollectGarbage(),
    CreateSystem(
        BTreeMap<SystemCallType, Syntax>,
        oneshot::Sender<(usize, Arc<Mutex<PrivateSystem>>)>,
    ),
    GetSystem(usize, oneshot::Sender<Option<Arc<Mutex<PrivateSystem>>>>),
    DropSystemHandle(usize, oneshot::Sender<bool>),
    CreateActor(
        /* handle: */ JoinHandle<()>,
        /* tx: */ mpsc::Sender<crate::actor::Message>,
        oneshot::Sender<(usize, mpsc::Sender<crate::actor::Message>)>,
    ),
    GetActor(
        usize,
        oneshot::Sender<Option<mpsc::Sender<crate::actor::Message>>>,
    ),
    CreateMessage(oneshot::Sender<(usize, mpsc::Sender<crate::actor::Message>)>),
    GetMessage(
        usize,
        oneshot::Sender<Option<mpsc::Sender<crate::actor::Message>>>,
    ),
    RecvMessage(usize, oneshot::Sender<Option<Syntax>>),
    Exit(oneshot::Sender<()>),
    RealExit(),
}

impl std::fmt::Debug for SystemActorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MarkUseSignal(signal_type, id) => {
                f.write_str(format!("MarkUseSignal({:?}, {})", signal_type, id).as_str())
            }
            Self::StartCollectGarbage() => f.write_str(format!("StartCollectGarbage()").as_str()),
            Self::StopCollectGarbage() => f.write_str(format!("StopCollectGarbage()").as_str()),
            Self::CreateSystem(map, _) => f.write_str(format!("CreateSystem({:?})", map).as_str()),
            Self::GetSystem(id, _) => f.write_str(format!("GetSystem({})", id).as_str()),
            Self::DropSystemHandle(id, _) => {
                f.write_str(format!("DropSystemHandle({})", id).as_str())
            }
            Self::CreateActor(_, _, _) => f.write_str(format!("CreateActor()").as_str()),
            Self::GetActor(id, _) => f.write_str(format!("GetActor({})", id).as_str()),
            Self::CreateMessage(_) => f.write_str(format!("CreateMessage()").as_str()),
            Self::GetMessage(id, _) => f.write_str(format!("GetMessage({})", id).as_str()),
            Self::RecvMessage(id, _) => f.write_str(format!("RecvMessage({})", id).as_str()),
            Self::Exit(_) => f.write_str("Exit()"),
            Self::RealExit() => f.write_str("RealExit()"),
        }
    }
}

struct ActorHandle {
    pub tx: mpsc::Sender<crate::actor::Message>,
    pub handle: JoinHandle<()>,
    pub used: bool,
}

impl ActorHandle {
    async fn destroy(self) -> JoinHandle<()> {
        let (tx, rx) = oneshot::channel();
        if let Ok(()) = self.tx.send(crate::actor::Message::Exit(tx)).await {
            rx.await;
        }

        self.handle
    }
}

struct MessageHandle {
    pub tx: mpsc::Sender<crate::actor::Message>,
    pub rx: mpsc::Receiver<crate::actor::Message>,
    pub used: bool,
}

struct SystemActor {
    systems: HashMap<usize, Arc<Mutex<PrivateSystem>>>,
    system_handles_count: usize,
    last_id: usize,
    actors: HashMap<usize, ActorHandle>,
    last_actor_id: usize,
    messages: HashMap<usize, MessageHandle>,
    last_message_id: usize,
    rx: mpsc::Receiver<SystemActorMessage>,
    tx: mpsc::Sender<SystemActorMessage>,
}

impl SystemActor {
    fn run() -> mpsc::Sender<SystemActorMessage> {
        let (tx, rx) = mpsc::channel(16);
        let tx_result = tx.clone();

        tokio::spawn(async move {
            let mut actor = Self {
                systems: [(
                    0,
                    Arc::new(Mutex::new(PrivateSystem::new(0, Default::default()))),
                )]
                .into_iter()
                .collect(),
                system_handles_count: 0,
                last_id: 0,
                actors: Default::default(),
                last_actor_id: 0,
                messages: Default::default(),
                last_message_id: 0,
                rx,
                tx,
            };

            actor.run_actor().await;
        });

        tx_result
    }

    async fn run_actor(mut self) {
        let mut exit_handle: Option<oneshot::Sender<()>> = None;
        while let Some(msg) = self.rx.recv().await {
            match msg {
                SystemActorMessage::CreateSystem(map, result) => {
                    result.send(self.create_system(map)).ok();
                }
                SystemActorMessage::GetSystem(id, result) => {
                    result.send(self.get_system(id)).ok();
                }
                SystemActorMessage::DropSystemHandle(id, result) => {
                    result.send(self.drop_system_handle(id).await).ok();
                }
                SystemActorMessage::CreateActor(handle, tx, result) => {
                    result.send(self.create_actor(handle, tx)).ok();
                }
                SystemActorMessage::GetActor(id, result) => {
                    result.send(self.get_actor(id)).ok();
                }
                SystemActorMessage::CreateMessage(result) => {
                    result.send(self.create_message()).ok();
                }
                SystemActorMessage::GetMessage(id, result) => {
                    result.send(self.get_message_sender(id)).ok();
                }
                SystemActorMessage::RecvMessage(id, result) => {
                    result.send(self.recv_message(id).await).ok();
                }
                SystemActorMessage::StartCollectGarbage() => {}
                SystemActorMessage::StopCollectGarbage() => {}
                SystemActorMessage::MarkUseSignal(signal, id) => {}
                SystemActorMessage::Exit(result) => {
                    exit_handle = Some(result);

                    let tx = self.tx.clone();
                    let actors_tx: Vec<mpsc::Sender<crate::actor::Message>> =
                        self.actors.values().map(|actor| actor.tx.clone()).collect();
                    tokio::spawn(async move {
                        log::debug!("Exiting {} actors", actors_tx.len());
                        let mut actors_wait_handlers = Vec::new();
                        for actor_tx in actors_tx {
                            let (tx, rx) = oneshot::channel();
                            if let Err(err) = actor_tx.send(crate::actor::Message::Exit(tx)).await {
                                error!("{}", err);
                            } else {
                                actors_wait_handlers.push(rx);
                            }
                        }

                        for actors_wait_handler in actors_wait_handlers {
                            actors_wait_handler.await;
                        }

                        log::debug!("Exited actors");

                        if let Err(err) = tx.send(SystemActorMessage::RealExit()).await {
                            error!("{}", err);
                        }
                    });
                }
                SystemActorMessage::RealExit() => break,
            }
        }

        log::debug!("Cleaning up system actor");
        self.drop_actors().await;
        self.messages.drain();
        self.systems.drain();
        log::debug!("Cleaned up system actor");

        if let Some(exit_handle) = exit_handle {
            exit_handle.send(());
        }
    }

    pub fn get_system(&mut self, id: usize) -> Option<Arc<Mutex<PrivateSystem>>> {
        self.systems.get_mut(&id).map(|system| system.clone())
    }

    pub fn create_system(
        &mut self,
        map: BTreeMap<SystemCallType, Syntax>,
    ) -> (usize, Arc<Mutex<PrivateSystem>>) {
        self.last_id += 1;
        let id = self.last_id;

        let system = Arc::new(Mutex::new(PrivateSystem::new(id, map)));
        self.systems.insert(id, system.clone());

        (id, system)
    }

    pub async fn drop_system_handle(&mut self, id: usize) -> bool {
        true
    }

    pub fn create_actor(
        &mut self,
        handle: JoinHandle<()>,
        tx: mpsc::Sender<crate::actor::Message>,
    ) -> (usize, mpsc::Sender<crate::actor::Message>) {
        let id = self.last_actor_id;
        self.last_actor_id += 1;

        self.actors.insert(
            id,
            ActorHandle {
                tx: tx.clone(),
                handle,
                used: false,
            },
        );

        (id, tx)
    }

    pub fn get_actor(&self, id: usize) -> Option<mpsc::Sender<crate::actor::Message>> {
        self.actors.get(&id).map(|actor| actor.tx.clone())
    }

    pub fn create_message(&mut self) -> (usize, mpsc::Sender<crate::actor::Message>) {
        let id = self.last_message_id;
        self.last_message_id += 1;

        let (tx, rx) = mpsc::channel(128);
        self.messages.insert(
            id,
            MessageHandle {
                tx: tx.clone(),
                rx,
                used: false,
            },
        );

        (id, tx)
    }

    pub async fn recv_message(&mut self, id: usize) -> Option<Syntax> {
        let rx = &mut self.messages.get_mut(&id)?.rx;

        rx.recv()
            .await
            .map(|msg| match msg {
                actor::Message::Signal(expr) => Some(expr),
                _ => None,
            })
            .flatten()
    }

    pub fn get_message_sender(&self, id: usize) -> Option<mpsc::Sender<crate::actor::Message>> {
        self.messages.get(&id).map(|msg| msg.tx.clone())
    }

    pub async fn drop_actors(&mut self) {
        debug!("Actors to clean up: {}", self.actors.len());
        if self.actors.is_empty() {
            return;
        }

        debug!("Dropping actors");
        // We have to return the actors, otherwise the runtime will be locked

        join_all(self.actors.drain().map(|(handle, actor)| async move {
            if let Err(err) = actor.destroy().await.await {
                error!("{}", err);
            }
        }))
        .await;

        debug!("Awaiting actor tasks");
    }
}

#[derive(Clone)]
pub struct SystemHandler {
    id: usize,
    system: mpsc::Sender<SystemActorMessage>,
    actors: HashMap<usize, mpsc::Sender<crate::actor::Message>>,
    messages: HashMap<usize, mpsc::Sender<crate::actor::Message>>,
}

impl Default for SystemHandler {
    fn default() -> SystemHandler {
        let system_holder = SystemActor::run();

        SystemHandler {
            id: 0,
            system: SystemActor::run(),
            actors: Default::default(),
            messages: Default::default(),
        }
    }
}

impl SystemHandler {
    pub fn get_id(&self) -> usize {
        self.id
    }

    pub async fn do_syscall(
        &mut self,
        ctx: &mut ContextHandler,
        system: &mut SystemHandler,
        no_change: bool,
        syscall: SystemCallType,
        expr: Syntax,
    ) -> Result<Syntax, InterpreterError> {
        self.get_system(self.id)
            .await
            .unwrap()
            .do_syscall(ctx, system, no_change, syscall, expr)
            .await
    }

    pub async fn get_handler(&mut self, id: usize) -> Option<SystemHandler> {
        let (tx, rx) = oneshot::channel();
        let get_system = self.system.send(SystemActorMessage::GetSystem(id, tx));
        let result = SystemHandler {
            id,
            system: self.system.clone(),
            actors: Default::default(),
            messages: Default::default(),
        };

        get_system.await.ok()?;

        match rx.await.ok()? {
            Some(_) => Some(result),
            None => None,
        }
    }

    pub async fn get_expr_for_syscall(&mut self, syscall: SystemCallType) -> Option<Syntax> {
        self.get_system(self.id).await?.get(syscall).await
    }

    pub async fn get_system(&mut self, id: usize) -> Option<System> {
        let (tx, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::GetSystem(id, tx))
            .await
            .ok()?;

        Some(System {
            id,
            private_system: rx.await.ok()??,
        })
    }

    pub async fn new_system(&mut self, map: BTreeMap<SystemCallType, Syntax>) -> usize {
        let (tx, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::CreateSystem(map, tx))
            .await
            .unwrap();

        rx.await.unwrap().0
    }

    pub async fn new_system_handler(
        &mut self,
        map: BTreeMap<SystemCallType, Syntax>,
    ) -> SystemHandler {
        let id = self.new_system(map).await;

        SystemHandler {
            id,
            system: self.system.clone(),
            messages: Default::default(),
            actors: Default::default(),
        }
    }

    pub async fn create_actor(
        &mut self,
        handle: JoinHandle<()>,
        tx: mpsc::Sender<crate::actor::Message>,
    ) -> usize {
        let (tx_result, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::CreateActor(handle, tx, tx_result))
            .await
            .unwrap();
        let (id, tx) = rx.await.unwrap();
        self.actors.insert(id, tx);

        id
    }

    pub async fn get_actor(&mut self, id: usize) -> Option<mpsc::Sender<crate::actor::Message>> {
        if let Some(tx) = self.actors.get(&id) {
            return Some(tx.clone());
        }

        let (tx_result, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::GetActor(id, tx_result))
            .await
            .ok()?;
        let tx = rx.await.ok()??;
        self.actors.insert(id, tx.clone());

        Some(tx)
    }

    pub async fn create_message(&mut self) -> usize {
        let (tx, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::CreateMessage(tx))
            .await
            .unwrap();
        let (id, tx) = rx.await.unwrap();
        self.messages.insert(id, tx);

        id
    }

    pub async fn send_message(&mut self, id: usize, expr: Syntax) -> anyhow::Result<()> {
        if let Some(tx) = self.messages.get(&id) {
            tx.send(crate::actor::Message::Signal(expr)).await?;

            Ok(())
        } else {
            let (tx, rx) = oneshot::channel();
            self.system
                .send(SystemActorMessage::GetMessage(id, tx))
                .await?;
            if let Some(tx) = rx.await? {
                self.messages.insert(id, tx.clone());

                tx.send(crate::actor::Message::Signal(expr)).await?;
                Ok(())
            } else {
                Err(anyhow::anyhow!("Message {} does not exist", id))
            }
        }
    }

    pub async fn recv_message(&mut self, id: usize) -> anyhow::Result<Syntax> {
        let (tx, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::RecvMessage(id, tx))
            .await?;

        Ok(rx.await.map_err(|err| anyhow!("{}", err))?.unwrap())
    }

    pub async fn exit(&mut self) {
        let (tx, rx) = oneshot::channel();
        if let Err(err) = self.system.send(SystemActorMessage::Exit(tx)).await {
            error!("{}", err);
        }
        rx.await;
    }
}
