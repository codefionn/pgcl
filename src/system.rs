use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::anyhow;
use futures::future::join_all;
use tokio::{
    sync::{mpsc, oneshot, Mutex},
    task::JoinHandle,
};

pub use crate::syscall::{PrivateSystem, SystemCallType};
use crate::{
    actor::{self, ActorHandle},
    context::ContextHandler,
    errors::InterpreterError,
    execute::{SignalType, Syntax},
    interpreter::LexerMessage,
    runner::{Runner, RunnerMessage},
    VerboseLevel,
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
        show_steps: VerboseLevel,
        debug: bool,
    ) -> Result<Option<Syntax>, InterpreterError> {
        self.private_system
            .lock()
            .await
            .do_syscall(
                ctx, system, runner, no_change, syscall, expr, show_steps, debug,
            )
            .await
    }

    pub async fn get(&self, syscall: SystemCallType) -> Option<Syntax> {
        self.private_system.lock().await.get(syscall).await
    }
}

enum SystemActorMessage {
    MarkUseSignal(SignalType, usize, /* result: */ oneshot::Sender<()>),
    CollectGarbage(Option<oneshot::Sender<()>>),
    FinishedCollectGarbage(),
    CreateSystem(
        BTreeMap<SystemCallType, Syntax>,
        oneshot::Sender<(usize, Arc<Mutex<PrivateSystem>>)>,
    ),
    GetSystem(usize, oneshot::Sender<Option<Arc<Mutex<PrivateSystem>>>>),
    CreateRunner(mpsc::Sender<RunnerMessage>, oneshot::Sender<usize>),
    DropRunner(usize),
    CreateActor(
        /* handle: */ JoinHandle<()>,
        /* tx: */ mpsc::Sender<crate::actor::Message>,
        /* running: */ Arc<AtomicBool>,
        oneshot::Sender<(usize, mpsc::Sender<crate::actor::Message>)>,
    ),
    GetActor(
        usize,
        oneshot::Sender<Option<mpsc::Sender<crate::actor::Message>>>,
    ),
    GetActorLen(oneshot::Sender<usize>),
    CreateMessage(oneshot::Sender<(usize, mpsc::Sender<crate::actor::Message>)>),
    GetMessage(
        usize,
        oneshot::Sender<Option<mpsc::Sender<crate::actor::Message>>>,
    ),
    RecvMessage(usize, oneshot::Sender<Option<Syntax>>),
    SetLexer(mpsc::Sender<LexerMessage>),
    Assert(Syntax, Syntax, Option<String>, bool),
    CountAsserts(oneshot::Sender<(usize, usize, usize)>),
    HasFailedAsserts(oneshot::Sender<bool>),
    Exit(oneshot::Sender<()>),
    RealExit(),
}

impl std::fmt::Debug for SystemActorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MarkUseSignal(signal_type, id, _) => {
                f.write_str(format!("MarkUseSignal({:?}, {})", signal_type, id).as_str())
            }
            Self::CollectGarbage(_) => f.write_str("CollectGarbage()"),
            Self::FinishedCollectGarbage() => f.write_str("FinishedCollectGarbage()"),
            Self::CreateSystem(map, _) => f.write_str(format!("CreateSystem({:?})", map).as_str()),
            Self::GetSystem(id, _) => f.write_str(format!("GetSystem({})", id).as_str()),
            Self::CreateActor(_, _, _, _) => f.write_str("CreateActor()"),
            Self::GetActor(id, _) => f.write_str(format!("GetActor({})", id).as_str()),
            Self::GetActorLen(_) => f.write_str("GetActorLen()"),
            Self::CreateMessage(_) => f.write_str("CreateMessage()"),
            Self::GetMessage(id, _) => f.write_str(format!("GetMessage({})", id).as_str()),
            Self::RecvMessage(id, _) => f.write_str(format!("RecvMessage({})", id).as_str()),
            Self::Exit(_) => f.write_str("Exit()"),
            Self::RealExit() => f.write_str("RealExit()"),
            Self::CreateRunner(_, _) => f.write_str("CreateRunner()"),
            Self::DropRunner(id) => f.write_str(format!("DropRunner({})", id).as_str()),
            Self::SetLexer(_) => f.write_str("SetLexer()"),
            Self::Assert(lhs, rhs, msg, success) => {
                if let Some(msg) = msg {
                    f.write_str(format!("RaiseAssert({lhs}, {rhs}, {msg}, {:?})", success).as_str())
                } else {
                    f.write_str(format!("RaiseAssert({lhs}, {rhs}, {:?})", success).as_str())
                }
            }
            Self::CountAsserts(_) => f.write_str("CountAssert()"),
            Self::HasFailedAsserts(_) => f.write_str("HasFailedAsserts()"),
        }
    }
}

struct MessageHandle {
    pub tx: mpsc::Sender<crate::actor::Message>,
    pub rx: mpsc::Receiver<crate::actor::Message>,
    pub used: bool,
}

struct Assertion {
    pub lhs: Syntax,
    pub rhs: Syntax,
    pub msg: Option<String>,
    pub success: bool,
}

struct SystemActor {
    systems: HashMap<usize, Arc<Mutex<PrivateSystem>>>,
    last_id: usize,
    actors: HashMap<usize, ActorHandle>,
    last_actor_id: usize,
    messages: HashMap<usize, MessageHandle>,
    last_message_id: usize,
    rx: mpsc::Receiver<SystemActorMessage>,
    tx: mpsc::Sender<SystemActorMessage>,
    runners: HashMap<usize, mpsc::Sender<RunnerMessage>>,
    last_runner_id: usize,
    tx_lexer: Option<mpsc::Sender<LexerMessage>>,
    gc_running: Option<JoinHandle<()>>,
    gc_result: Vec<oneshot::Sender<()>>,
    gc_even: bool,
    running: Arc<AtomicBool>,
    exit_handles: Vec<oneshot::Sender<()>>,
    assertions: Vec<Assertion>,
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
                last_id: 0,
                actors: Default::default(),
                last_actor_id: 0,
                messages: Default::default(),
                last_message_id: 0,
                runners: Default::default(),
                last_runner_id: 0,
                gc_running: None,
                gc_even: true,
                gc_result: Default::default(),
                rx,
                tx,
                tx_lexer: None,
                running: Arc::new(AtomicBool::new(true)),
                exit_handles: Default::default(),
                assertions: Vec::new(),
            };

            actor.run_actor().await;
        });

        tx_result
    }

    async fn run_actor(mut self) {
        //#[cfg(debug_assertions)]
        //debug!("System actor: Started");

        tokio::spawn({
            let tx = self.tx.clone();
            let running = self.running.clone();
            async move {
                while running.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    let (tx_result, rx_result) = oneshot::channel();
                    if tx
                        .send(SystemActorMessage::CollectGarbage(Some(tx_result)))
                        .await
                        .is_err()
                    {
                        break;
                    }

                    let _ = rx_result.await;
                }
            }
        });

        while let Some(msg) = self.rx.recv().await {
            if self.handle_messsage(msg).await {
                break;
            }
        }

        self.running.swap(false, Ordering::Relaxed);
        self.await_gc().await;

        if let Some(tx_lexer) = &mut self.tx_lexer {
            tx_lexer.send(LexerMessage::RealExit()).await.ok();
        }

        //#[cfg(debug_assertions)]
        //log::debug!("Cleaning up system actor");
        self.drop_actors().await;
        self.messages.drain();
        self.systems.drain();
        //#[cfg(debug_assertions)]
        //log::debug!("Cleaned up system actor");

        for exit_handle in self.exit_handles {
            if let Err(err) = exit_handle.send(()) {
                log::error!("exit: {:?}", err);
            }
        }
    }

    pub async fn handle_messsage(&mut self, msg: SystemActorMessage) -> bool {
        //#[cfg(debug_assertions)]
        //debug!("{:?}", msg);

        match msg {
            SystemActorMessage::CreateSystem(map, result) => {
                result.send(self.create_system(map)).ok();
            }
            SystemActorMessage::GetSystem(id, result) => {
                result.send(self.get_system(id)).ok();
            }
            SystemActorMessage::CreateActor(handle, tx, running, result) => {
                result.send(self.create_actor(handle, tx, running)).ok();
            }
            SystemActorMessage::GetActor(id, result) => {
                result.send(self.get_actor(id)).ok();
            }
            SystemActorMessage::GetActorLen(result) => {
                result.send(self.actors.len()).ok();
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
            SystemActorMessage::CollectGarbage(result) => {
                if let Some(result) = result {
                    self.gc_result.push(result);
                }

                self.gc();
            }
            SystemActorMessage::FinishedCollectGarbage() => {
                self.await_gc().await;
            }
            SystemActorMessage::MarkUseSignal(signal, id, result) => {
                self.mark_use_signal(signal, id);
                let _ = result.send(());
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
            SystemActorMessage::Assert(lhs, rhs, msg, success) => {
                self.assertions.push(Assertion {
                    lhs,
                    rhs,
                    msg,
                    success,
                });
            }
            SystemActorMessage::CountAsserts(result) => {
                let asserts_success = self.assertions.iter().filter(|a| a.success).count();

                result
                    .send((
                        self.assertions.len(),
                        asserts_success,
                        self.assertions.len() - asserts_success,
                    ))
                    .ok();
            }
            SystemActorMessage::HasFailedAsserts(result) => {
                result
                    .send(self.assertions.iter().any(|assertion| !assertion.success))
                    .ok();
            }
            SystemActorMessage::Exit(result) => {
                self.exit_handles.push(result);

                let tx = self.tx.clone();
                /*let actors_tx: Vec<mpsc::Sender<crate::actor::Message>> =
                self.actors.values().map(|actor| actor.tx.clone()).collect();*/

                let running = self.running.clone();
                tokio::spawn(async move {
                    {
                        let (tx_result, rx_result) = oneshot::channel();
                        if tx
                            .send(SystemActorMessage::CollectGarbage(Some(tx_result)))
                            .await
                            .is_ok()
                        {
                            let _ = rx_result.await;
                        }

                        running.swap(false, Ordering::Relaxed);
                    }

                    while {
                        let (tx_result, rx_result) = oneshot::channel();
                        let _ = tx.send(SystemActorMessage::GetActorLen(tx_result)).await;
                        rx_result.await.unwrap() > 0
                    } {
                        let (tx_result, rx_result) = oneshot::channel();
                        if tx
                            .send(SystemActorMessage::CollectGarbage(Some(tx_result)))
                            .await
                            .is_ok()
                        {
                            let _ = rx_result.await;
                        }
                    }

                    /*#[cfg(debug_assertions)]
                    log::debug!("Exiting {} actors", actors_tx.len());
                    let mut actors_wait_handlers = Vec::new();
                    for actor_tx in actors_tx {
                        let (tx, rx) = oneshot::channel();
                        if let Err(err) = actor_tx.send(crate::actor::Message::Exit(tx)).await {
                            log::error!("exit-actor: {}", err);
                        } else {
                            actors_wait_handlers.push(rx);
                        }
                    }

                    for actors_wait_handler in actors_wait_handlers {
                        let _ = actors_wait_handler.await;
                    }

                    //#[cfg(debug_assertions)]
                    //log::debug!("Exited actors");*/

                    if let Err(err) = tx.send(SystemActorMessage::RealExit()).await {
                        log::error!("real-exit: {}", err);
                    }
                });
            }
            SystemActorMessage::RealExit() => {
                return true;
            }
        }

        false
    }

    pub fn mark_use_signal(&mut self, signal_type: SignalType, id: usize) {
        //#[cfg(debug_assertions)]
        //debug!("mark_use_signal({:?}, {})", signal_type, id);

        match signal_type {
            SignalType::Actor => {
                if let Some(actor) = self.actors.get_mut(&id) {
                    actor.used = true;
                }
            }
            SignalType::Message => {
                if let Some(msg) = self.messages.get_mut(&id) {
                    msg.used = true;
                }
            }
        }
    }

    pub async fn await_gc(&mut self) -> Option<()> {
        let gc_running = self.gc_running.as_mut()?;
        let _ = gc_running.await;

        //#[cfg(debug_assertions)]
        //debug!("GC: Finishing up");

        let mut to_drop_actors_ids: Vec<usize> = Vec::new();
        for (id, actor) in &self.actors {
            if !actor.used && !actor.running.load(Ordering::Relaxed) {
                // When an actor was started
                // during collection do this
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

        let mut to_drop_messages: Vec<MessageHandle> =
            Vec::with_capacity(to_drop_message_ids.len());
        for msg_id in to_drop_message_ids {
            to_drop_messages.push(self.messages.remove(&msg_id).unwrap());
        }

        tokio::spawn(async move {
            #[cfg(debug_assertions)]
            log::trace!(
                "GC: {} actors, {} messages",
                to_drop_actors.len(),
                to_drop_messages.len()
            );

            for actor in to_drop_actors.into_iter() {
                actor.destroy().await;
            }

            for msg in to_drop_messages {
                std::mem::drop(msg);
            }
        });

        for gc_result in self.gc_result.drain(..) {
            if let Err(err) = gc_result.send(()) {
                log::error!("gc-exit: {:?}", err);
            }
        }

        self.gc_running = None;

        Some(())
    }

    pub fn gc(&mut self) {
        if self.gc_running.is_some() {
            return;
        }

        //#[cfg(debug_assertions)]
        //debug!("GC: Started");

        // Mark everying as not used
        for actor in self.actors.values_mut() {
            actor.used = actor.running.load(Ordering::Relaxed);
        }

        for msg in self.messages.values_mut() {
            msg.used = false;
        }

        let tx = self.tx.clone();
        let runners_tx: Vec<mpsc::Sender<RunnerMessage>> = self.runners.values().cloned().collect();
        let mut tx_lexer = self.tx_lexer.clone();
        let mut actors: Vec<mpsc::Sender<crate::actor::Message>> = self
            .actors
            .values_mut()
            .map(|actor| actor.tx.clone())
            .collect();

        let gc_even = self.gc_even;
        self.gc_even = !gc_even;
        self.gc_running = Some(tokio::spawn(async move {
            //#[cfg(debug_assertions)]
            //debug!("GC: Waiting for {} runners", runners_tx.len());

            let runners_tx = join_all(runners_tx.into_iter().map(|tx| async move {
                let (result_tx, result_rx) = oneshot::channel();
                let _ = tx.send(RunnerMessage::Mark(gc_even, result_tx)).await;

                (tx, result_rx)
            }))
            .await;

            let runners = tokio::spawn(async move {
                join_all(
                    runners_tx
                        .into_iter()
                        .map(|(tx, result_rx)| async move {
                            let _ = result_rx.await;

                            tx
                        })
                        .map(|tx| async move {
                            let tx = tx.await;

                            let (result_tx, result_rx) = oneshot::channel();
                            if tx.send(RunnerMessage::Sweep(result_tx)).await.is_ok() {
                                let _ = result_rx.await;
                            }
                        }),
                )
                .await;
            });

            while !runners.is_finished() {
                tokio::time::sleep(Duration::from_millis(30)).await;

                if let Some(tx_lexer) = tx_lexer.as_mut() {
                    let _ = tx_lexer.send(LexerMessage::Wakeup()).await;
                }

                for actor in &mut actors {
                    let _ = actor.send(crate::actor::Message::Wakeup()).await;
                }
            }

            if let Err(err) = runners.await {
                log::error!("gc-runners-await: {}", err);
            }

            tx.send(SystemActorMessage::FinishedCollectGarbage())
                .await
                .ok();
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

    pub fn create_actor(
        &mut self,
        handle: JoinHandle<()>,
        tx: mpsc::Sender<crate::actor::Message>,
        running: Arc<AtomicBool>,
    ) -> (usize, mpsc::Sender<crate::actor::Message>) {
        let id = self.last_actor_id;
        self.last_actor_id += 1;

        self.actors.insert(
            id,
            ActorHandle {
                tx: tx.clone(),
                handle,
                used: true,
                running,
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

        rx.recv().await.and_then(|msg| match msg {
            actor::Message::Signal(expr) => Some(expr),
            _ => None,
        })
    }

    pub fn get_message_sender(&self, id: usize) -> Option<mpsc::Sender<crate::actor::Message>> {
        self.messages.get(&id).map(|msg| msg.tx.clone())
    }

    pub async fn drop_actors(&mut self) {
        //#[cfg(debug_assertions)]
        //debug!("Actors to clean up: {}", self.actors.len());
        if self.actors.is_empty() {
            return;
        }

        //#[cfg(debug_assertions)]
        //debug!("Dropping actors");

        // We have to return the actors, otherwise the runtime will be locked

        join_all(self.actors.drain().map(|(_, actor)| async move {
            if let Err(err) = actor.destroy().await.await {
                log::error!("destroy-actor: {}", err);
            }
        }))
        .await;

        //#[cfg(debug_assertions)]
        //debug!("Awaiting actor tasks");
    }

    pub fn create_runner(&mut self, tx: mpsc::Sender<RunnerMessage>) -> usize {
        let id = self.last_runner_id;
        self.last_runner_id += 1;

        if self.runners.insert(id, tx).is_some() {
            panic!("There should not be any runner with id {}", id);
        }

        id
    }

    pub fn drop_runner(&mut self, id: usize) {
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
        show_steps: VerboseLevel,
        debug: bool,
    ) -> Result<Option<Syntax>, InterpreterError> {
        self.get_system(self.id)
            .await
            .unwrap()
            .do_syscall(
                ctx, system, runner, no_change, syscall, expr, show_steps, debug,
            )
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

        rx.await.ok()?.map(|_| result)
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
        running: Arc<AtomicBool>,
    ) -> usize {
        let (tx_result, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::CreateActor(
                handle, tx, running, tx_result,
            ))
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

        Ok(rx
            .await
            .map_err(|err| anyhow!("recv_message: {}", err))?
            .unwrap())
    }

    pub async fn set_lexer(&mut self, tx_lexer: mpsc::Sender<LexerMessage>) {
        if let Err(err) = self
            .system
            .send(SystemActorMessage::SetLexer(tx_lexer))
            .await
        {
            log::error!("set_lexer: {}", err);
        }
    }

    pub async fn exit(&mut self) {
        let (tx, rx) = oneshot::channel();
        if let Err(err) = self.system.send(SystemActorMessage::Exit(tx)).await {
            log::error!("send-exit: {}", err);
        } else if let Err(err) = rx.await {
            log::error!("send-exit-await: {}", err);
        }
    }

    pub async fn mark_use_signal(&mut self, signal_type: SignalType, id: usize) {
        let (tx, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::MarkUseSignal(signal_type, id, tx))
            .await
            .ok();

        if let Err(err) = rx.await {
            log::error!("mark_use_signal: {err}");
        }
    }

    pub async fn create_runner(
        &mut self,
        tx_runner: mpsc::Sender<RunnerMessage>,
    ) -> anyhow::Result<usize> {
        let (tx, rx) = oneshot::channel();
        self.system
            .send(SystemActorMessage::CreateRunner(tx_runner, tx))
            .await?;

        let result = rx.await?;

        Ok(result)
    }

    pub async fn drop_runner(&mut self, id: usize) {
        self.system
            .send(SystemActorMessage::DropRunner(id))
            .await
            .ok();
    }

    pub async fn add_assert(
        &mut self,
        lhs: Syntax,
        rhs: Syntax,
        msg: Option<String>,
        success: bool,
    ) {
        self.system
            .send(SystemActorMessage::Assert(lhs, rhs, msg, success))
            .await
            .ok();
    }

    pub async fn has_failed_asserts(&mut self) -> bool {
        let (tx, rx) = oneshot::channel();
        match self
            .system
            .send(SystemActorMessage::HasFailedAsserts(tx))
            .await
        {
            Ok(_) => rx.await.unwrap_or(false),
            Err(_) => false,
        }
    }

    pub async fn count_assertions(&mut self) -> Result<(usize, usize, usize), InterpreterError> {
        let (tx, rx) = oneshot::channel();
        match self.system.send(SystemActorMessage::CountAsserts(tx)).await {
            Ok(()) => rx.await.map_err(|_| InterpreterError::UnknownError()),
            Err(_) => Err(InterpreterError::UnknownError()),
        }
    }
}
