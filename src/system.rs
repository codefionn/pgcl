use std::{
    collections::{BTreeMap, HashMap}, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration, ops::Deref,
};

use anyhow::anyhow;
use futures::{future::join_all, SinkExt};
use log::{debug, error};
use tokio::{
    sync::{mpsc, oneshot, Mutex, RwLock},
    task::JoinHandle,
};

pub use crate::syscall::{PrivateSystem, SystemCallType};
use crate::{
    actor::{self, ActorHandle},
    context::ContextHandler,
    errors::InterpreterError,
    execute::{SignalType, Syntax}, runner::{RunnerMessage, Runner}, interpreter::LexerMessage,
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
        runner: &mut Runner,
        no_change: bool,
        syscall: SystemCallType,
        expr: Syntax,
        show_steps: bool,
    ) -> Result<Syntax, InterpreterError> {
        self.private_system
            .lock()
            .await
            .do_syscall(ctx, system, runner, no_change, syscall, expr, show_steps)
            .await
    }

    pub async fn get(&self, syscall: SystemCallType) -> Option<Syntax> {
        self.private_system.lock().await.get(syscall).await
    }
}

pub enum SystemHandleMessage {}

enum SystemActorMessage {
    MarkUseSignal(SignalType, usize, /* result: */ oneshot::Sender<()>),
    CollectGarbage(),
    FinishedCollectGarbage(),
    CreateSystem(
        BTreeMap<SystemCallType, Syntax>,
        oneshot::Sender<(usize, Arc<Mutex<PrivateSystem>>)>,
    ),
    GetSystem(usize, oneshot::Sender<Option<Arc<Mutex<PrivateSystem>>>>),
    DropSystemHandle(usize, oneshot::Sender<bool>),
    CreateRunner(mpsc::Sender<RunnerMessage>, oneshot::Sender<usize>),
    DropRunner(usize),
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
    SetLexer(mpsc::Sender<LexerMessage>),
    Exit(oneshot::Sender<()>),
    RealExit(),
}

impl std::fmt::Debug for SystemActorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MarkUseSignal(signal_type, id, _) => {
                f.write_str(format!("MarkUseSignal({:?}, {})", signal_type, id).as_str())
            }
            Self::CollectGarbage() => f.write_str(format!("CollectGarbage()").as_str()),
            Self::FinishedCollectGarbage() => f.write_str(format!("FinishedCollectGarbage()").as_str()),
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
            Self::CreateRunner(_, _) => f.write_str("CreateRunner()"),
            Self::DropRunner(id) => f.write_str(format!("DropRunner({})", id).as_str()),
            Self::SetLexer(_) => f.write_str(format!("SetLexer()").as_str()),
        }
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
    runners: HashMap<usize, mpsc::Sender<RunnerMessage>>,
    last_runner_id: usize,
    gc_running: Option<JoinHandle<()>>,
    tx_lexer: Option<mpsc::Sender<LexerMessage>>
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
                runners: Default::default(),
                last_runner_id: 0,
                gc_running: None,
                rx,
                tx,
                tx_lexer: None
            };

            actor.run_actor().await;
        });

        tx_result
    }

    async fn run_actor(mut self) {
        #[cfg(debug_assertions)]
        debug!("System actor: Started");

        let mut exit_handle: Option<oneshot::Sender<()>> = None;
        let mut running = Arc::new(AtomicBool::new(true));
        let mut watcher = tokio::spawn({
            let tx = self.tx.clone();
            let mut running = running.clone();
            async move {
                while running.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    if let Err(_) = tx.send(SystemActorMessage::CollectGarbage()).await {
                        break;
                    }
                }
            }
        });

        while let Some(msg) = self.rx.recv().await {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            #[cfg(debug_assertions)]
            debug!("{:?}", msg);

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
                SystemActorMessage::CollectGarbage() => {
                    self.gc();
                }
                SystemActorMessage::FinishedCollectGarbage() => {
                    self.await_gc().await;
                }
                SystemActorMessage::MarkUseSignal(signal, id, result) => {
                    self.mark_use_signal(signal, id);
                    result.send(());
                }
                SystemActorMessage::CreateRunner(tx, result) => {
                    result.send(self.create_runner(tx)).ok();
                }
                SystemActorMessage::DropRunner(id) => {
                    self.drop_runner(id);
                }
                SystemActorMessage::SetLexer(tx_lexer) => {
                    self.tx_lexer = Some(tx_lexer);
                }
                SystemActorMessage::Exit(result) => {
                    running.swap(false, Ordering::Relaxed);

                    exit_handle = Some(result);

                    let tx = self.tx.clone();
                    let actors_tx: Vec<mpsc::Sender<crate::actor::Message>> =
                        self.actors.values().map(|actor| actor.tx.clone()).collect();
                    tokio::spawn(async move {
                        tx.send(SystemActorMessage::CollectGarbage()).await.ok();

                        #[cfg(debug_assertions)]
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

                        #[cfg(debug_assertions)]
                        log::debug!("Exited actors");

                        if let Err(err) = tx.send(SystemActorMessage::RealExit()).await {
                            error!("{}", err);
                        }
                    });
                }
                SystemActorMessage::RealExit() => break,
            }
        }

        running.swap(false, Ordering::Relaxed);
        self.await_gc().await;

        if let Some(tx_lexer) = &mut self.tx_lexer {
            tx_lexer.send(LexerMessage::RealExit()).await.ok();
        }

        #[cfg(debug_assertions)]
        log::debug!("Cleaning up system actor");
        self.drop_actors().await;
        self.messages.drain();
        self.systems.drain();
        #[cfg(debug_assertions)]
        log::debug!("Cleaned up system actor");

        if let Some(exit_handle) = exit_handle {
            exit_handle.send(());
        }
    }

    pub fn mark_use_signal(&mut self, signal_type: SignalType, id: usize) {
        #[cfg(debug_assertions)]
        debug!("mark_use_signal({:?}, {})", signal_type, id);

        match signal_type {
            SignalType::Actor => {
                if let Some(actor) = self.actors.get_mut(&id) {
                    actor.used = true;
                }
            },
            SignalType::Message => {
                if let Some(msg) = self.messages.get_mut(&id) {
                     msg.used = true;
                }
            }
        }
    }

    pub async fn await_gc(&mut self) -> Option<()> {
        let gc_running = self.gc_running.as_mut()?;
        gc_running.await;

        #[cfg(debug_assertions)]
        debug!("GC: Finishing up");

        let mut to_drop_actors_ids: Vec<usize> = Vec::new();
        for (id, actor) in &self.actors {
            if !actor.used {
                to_drop_actors_ids.push(*id);
            }
        }

        let mut to_drop_actors: Vec<ActorHandle> = Vec::with_capacity(to_drop_actors_ids.len());
        for actor_id in to_drop_actors_ids {
            to_drop_actors.push(self.actors.remove(&actor_id).unwrap());
        }

        let mut to_drop_message_ids: Vec<usize> = Vec::new();
        for (id, message) in &self.messages {
            if !message.used {
                to_drop_message_ids.push(*id);
            }
        }

        let mut to_drop_messages: Vec<MessageHandle> = Vec::with_capacity(to_drop_message_ids.len());
        for msg_id in to_drop_message_ids {
            to_drop_messages.push(self.messages.remove(&msg_id).unwrap());
        }

        tokio::spawn(async move {
            #[cfg(debug_assertions)]
            log::debug!("GC: {} actors, {} messages", to_drop_actors.len(), to_drop_messages.len());

            for actor in to_drop_actors.into_iter() {
                actor.destroy().await;
            }

            for msg in to_drop_messages {
                // Do nothing
            }
        });

        self.gc_running = None;

        Some(())
    }

    pub fn gc(&mut self) {
        if self.gc_running.is_some() {
            return;
        }

        #[cfg(debug_assertions)]
        debug!("GC: Started");

        // Mark everying as not used
        for actor in self.actors.values_mut() {
            actor.used = false;
        }

        for msg in self.messages.values_mut() {
            msg.used = false;
        }

        let tx = self.tx.clone();
        let runners_tx: Vec<mpsc::Sender<RunnerMessage>> = self.runners.values().into_iter().cloned().collect();
        let mut tx_lexer = self.tx_lexer.clone();
        let mut actors: Vec<mpsc::Sender<crate::actor::Message>> = self.actors.values_mut().map(|actor| actor.tx.clone()).collect();
        self.gc_running = Some(tokio::spawn(async move {
            #[cfg(debug_assertions)]
            debug!("GC: Waiting for {} runners", runners_tx.len());

            let runners_tx = join_all(runners_tx.into_iter().map(|tx| async move {
                let (result_tx, result_rx) = oneshot::channel();
                tx.send(RunnerMessage::Mark(result_tx)).await;
                
                (tx, result_rx)
            })).await;

            let runners = tokio::spawn(async move {
                join_all(runners_tx.into_iter().map(|(tx, result_rx)| async move {
                    result_rx.await;

                    tx
                })
                .map(|tx| async move {
                    let tx = tx.await;

                    let (result_tx, result_rx) = oneshot::channel();
                    tx.send(RunnerMessage::Sweep(result_tx)).await.ok();

                    result_rx.await;
                })).await;
            });

            while !runners.is_finished() {
                tokio::time::sleep(Duration::from_millis(30)).await;

                if let Some(tx_lexer) = tx_lexer.as_mut() {
                    tx_lexer.send(LexerMessage::Wakeup()).await;
                }

                for actor in &mut actors {
                    actor.send(crate::actor::Message::Wakeup()).await;
                }
            }

            runners.await;

            tx.send(SystemActorMessage::FinishedCollectGarbage()).await.ok();
        }));
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
                used: true,
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
                used: true,
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
        #[cfg(debug_assertions)]
        debug!("Actors to clean up: {}", self.actors.len());
        if self.actors.is_empty() {
            return;
        }

        #[cfg(debug_assertions)]
        debug!("Dropping actors");
        // We have to return the actors, otherwise the runtime will be locked

        join_all(self.actors.drain().map(|(handle, actor)| async move {
            if let Err(err) = actor.destroy().await.await {
                error!("{}", err);
            }
        }))
        .await;

        #[cfg(debug_assertions)]
        debug!("Awaiting actor tasks");
    }

    pub fn create_runner(&mut self, tx: mpsc::Sender<RunnerMessage>) -> usize {
        let id = self.last_runner_id;
        self.last_runner_id += 1;

        if self.runners.insert(id, tx).is_some() {
            panic!("There should not be any runner with id {}", id);
        }

        id
    }

    pub fn drop_runner(&mut self,  id: usize) {
        self.runners.remove(&id);
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
        runner: &mut Runner,
        no_change: bool,
        syscall: SystemCallType,
        expr: Syntax,
        show_steps: bool,
    ) -> Result<Syntax, InterpreterError> {
        self.get_system(self.id)
            .await
            .unwrap()
            .do_syscall(ctx, system, runner, no_change, syscall, expr, show_steps)
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

    pub async fn set_lexer(&mut self, tx_lexer: mpsc::Sender<LexerMessage>) {
        self.system.send(SystemActorMessage::SetLexer(tx_lexer)).await;
    }

    pub async fn exit(&mut self) {
        let (tx, rx) = oneshot::channel();
        if let Err(err) = self.system.send(SystemActorMessage::Exit(tx)).await {
            error!("{}", err);
        }
        rx.await;
    }

    pub async fn mark_use_signal(&mut self, signal_type: SignalType, id: usize) {
        let (tx, rx) = oneshot::channel();
        self.system.send(SystemActorMessage::MarkUseSignal(signal_type, id, tx)).await.ok();
        rx.await;
    }

    pub async fn create_runner(&mut self, tx_runner: mpsc::Sender<RunnerMessage>) -> anyhow::Result<usize> {
        let (tx, rx) = oneshot::channel();
        self.system.send(SystemActorMessage::CreateRunner(tx_runner, tx)).await?;

        let result = rx.await?;

        Ok(result)
    }

    pub async fn drop_runner(&mut self, id: usize) {
        self.system.send(SystemActorMessage::DropRunner(id)).await.ok();
    }
}
