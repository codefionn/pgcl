use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use bigdecimal::BigDecimal;
use num::FromPrimitive;
use tokio::{
    process::Command,
    sync::{mpsc, Mutex},
    time::Instant,
};

use crate::{
    actor,
    context::ContextHandler,
    errors::InterpreterError,
    execute::{SignalType, Syntax},
    rational::BigRational,
};

#[derive(Clone, Debug, PartialEq, Hash, Eq, PartialOrd, Ord, Copy)]
pub enum SystemCallType {
    Typeof,
    MeasureTime,
    Cmd,
    Println,
    Actor,
    ExitActor,
}

impl SystemCallType {
    pub const fn to_systemcall(&self) -> &'static str {
        match self {
            Self::Typeof => "type",
            Self::MeasureTime => "time",
            Self::Cmd => "cmd",
            Self::Println => "println",
            Self::Actor => "actor",
            Self::ExitActor => "exitactor",
        }
    }

    pub fn all() -> &'static [SystemCallType] {
        &[
            Self::Typeof,
            Self::MeasureTime,
            Self::Cmd,
            Self::Println,
            Self::Actor,
            Self::ExitActor,
        ]
    }
}

struct PrivateSystem {
    id: usize,
    map: BTreeMap<SystemCallType, Syntax>,
}

impl PrivateSystem {
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
        if let Some(expr) = self.map.get(&syscall) {
            return Ok(expr.clone());
        }

        Ok(match (syscall, expr) {
            (SystemCallType::Typeof, expr) => {
                fn create_syscall_type(expr: Box<Syntax>) -> Syntax {
                    Syntax::Call(
                        Box::new(Syntax::Id("syscall".to_string())),
                        Box::new(Syntax::Tuple(
                            Box::new(Syntax::ValAtom("type".to_string())),
                            expr,
                        )),
                    )
                }

                match expr {
                    Syntax::ValInt(_) => Syntax::ValAtom("int".to_string()),
                    Syntax::ValFlt(_) => Syntax::ValAtom("float".to_string()),
                    Syntax::ValStr(_) => Syntax::ValAtom("string".to_string()),
                    Syntax::Lst(_) => Syntax::ValAtom("list".to_string()),
                    Syntax::Map(_) => Syntax::ValAtom("map".to_string()),
                    Syntax::Lambda(_, _) => Syntax::ValAtom("lambda".to_string()),
                    Syntax::Tuple(lhs, rhs) => Syntax::Call(
                        Box::new(Syntax::ValAtom("tuple".to_string())),
                        Box::new(Syntax::Tuple(
                            Box::new(create_syscall_type(lhs)),
                            Box::new(create_syscall_type(rhs)),
                        )),
                    ),
                    expr @ _ => {
                        ctx.push_error(format!("Cannot infer type from: {}", expr))
                            .await;

                        Syntax::UnexpectedArguments()
                    }
                }
            }
            (SystemCallType::MeasureTime, expr) => {
                let now = Instant::now();

                let expr = expr.execute(false, ctx, system).await?;

                let diff = now.elapsed().as_secs_f64();
                let diff: BigRational = BigRational::from_f64(diff).unwrap();

                Syntax::Tuple(Box::new(Syntax::ValFlt(diff)), Box::new(expr))
            }
            (SystemCallType::Cmd, Syntax::ValStr(cmd)) => if cfg!(target_os = "windows") {
                Command::new("cmd").args(["/C", cmd.as_str()]).output()
            } else {
                Command::new("sh").args(["-c", cmd.as_str()]).output()
            }
            .await
            .map(|out| {
                let status = out.status.code().unwrap_or(1);
                let stdout = String::from_utf8(out.stdout).unwrap_or(String::new());
                let stderr = String::from_utf8(out.stderr).unwrap_or(String::new());

                let mut map = BTreeMap::new();
                map.insert("status".to_string(), (Syntax::ValInt(status.into()), true));
                map.insert("stdout".to_string(), (Syntax::ValStr(stdout), true));
                map.insert("stderr".to_string(), (Syntax::ValStr(stderr), true));

                Syntax::Map(map.into_iter().collect())
            })
            .unwrap_or(Syntax::ValAtom("error".to_string())),
            (SystemCallType::Println, Syntax::ValStr(s)) => {
                println!("{}", s);

                Syntax::ValAny()
            }
            (SystemCallType::Println, Syntax::ValInt(i)) => {
                println!("{}", i);

                Syntax::ValAny()
            }
            (SystemCallType::Println, Syntax::ValFlt(f)) => {
                let f: BigDecimal = f.into();
                println!("{}", f);

                Syntax::ValAny()
            }
            (SystemCallType::Actor, Syntax::Tuple(init, actor_fn)) => {
                let tx =
                    crate::actor::create_actor(ctx.clone(), system.clone(), *init, *actor_fn).await;
                let id = system.get_holder().create_actor(tx).await;

                Syntax::Signal(SignalType::Actor, id)
            }
            (SystemCallType::ExitActor, Syntax::Signal(signal_type, signal_id)) => {
                match signal_type {
                    SignalType::Actor => match system.get_holder().get_actor(signal_id).await {
                        Some(tx) => match tx.send(actor::Message::Exit()).await {
                            Ok(_) => Syntax::ValAtom("true".to_string()),
                            Err(_) => Syntax::ValAtom("false".to_string()),
                        },
                        _ => Syntax::ValAtom("false".to_string()),
                    },
                }
            }
            (syscall, expr) => Syntax::Call(
                Box::new(Syntax::Id("syscall".to_string())),
                Box::new(Syntax::Tuple(
                    Box::new(Syntax::ValAtom(syscall.to_systemcall().to_string())),
                    Box::new(expr.execute_once(false, no_change, ctx, system).await?),
                )),
            ),
        })
    }

    pub async fn get(&self, syscall: SystemCallType) -> Option<Syntax> {
        self.map.get(&syscall).map(|expr| expr.clone())
    }
}

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

struct PrivateSystemHolder {
    systems: HashMap<usize, Arc<Mutex<PrivateSystem>>>,
    last_id: usize,
    actors: HashMap<usize, mpsc::Sender<crate::actor::Message>>,
    last_actor_id: usize,
}

impl Default for PrivateSystemHolder {
    fn default() -> Self {
        Self {
            systems: [(
                0,
                Arc::new(Mutex::new(PrivateSystem {
                    id: 0,
                    map: Default::default(),
                })),
            )]
            .into_iter()
            .collect(),
            last_id: 0,
            actors: Default::default(),
            last_actor_id: 0,
        }
    }
}

impl PrivateSystemHolder {
    pub fn get(&mut self, id: usize) -> Option<Arc<Mutex<PrivateSystem>>> {
        self.systems.get_mut(&id).map(|system| system.clone())
    }

    pub fn new_system(&mut self, map: BTreeMap<SystemCallType, Syntax>) -> usize {
        self.last_id += 1;
        let id = self.last_id;

        let system = Arc::new(Mutex::new(PrivateSystem { id, map }));
        self.systems.insert(id, system);

        id
    }

    pub fn create_actor(&mut self, tx: mpsc::Sender<crate::actor::Message>) -> usize {
        let id = self.last_actor_id;
        self.last_actor_id += 1;

        self.actors.insert(id, tx);

        id
    }

    pub fn get_actor(&self, id: usize) -> Option<mpsc::Sender<crate::actor::Message>> {
        self.actors.get(&id).map(|tx| tx.clone())
    }

    pub async fn drop_actors(&mut self) {
        for (_, actor_tx) in &self.actors {
            actor_tx.send(crate::actor::Message::Exit()).await.ok();
        }

        self.actors.clear();
    }
}

#[derive(Clone)]
pub struct SystemHolder {
    system: Arc<Mutex<PrivateSystemHolder>>,
    actors: HashMap<usize, mpsc::Sender<crate::actor::Message>>,
}

impl Default for SystemHolder {
    fn default() -> Self {
        SystemHolder {
            system: Arc::new(Mutex::new(PrivateSystemHolder::default())),
            actors: Default::default(),
        }
    }
}

impl SystemHolder {
    pub async fn get(&mut self, id: usize) -> Option<System> {
        if let Some(system) = self.system.lock().await.get(id) {
            Some(System {
                id,
                private_system: system,
            })
        } else {
            None
        }
    }

    pub async fn get_handler(&mut self, id: usize) -> Option<SystemHandler> {
        if let Some(_) = self.get(id).await {
            Some(SystemHandler {
                id,
                system: self.clone(),
            })
        } else {
            None
        }
    }

    pub async fn new_system(&mut self, map: BTreeMap<SystemCallType, Syntax>) -> usize {
        self.system.lock().await.new_system(map)
    }

    pub async fn new_system_handler(
        &mut self,
        map: BTreeMap<SystemCallType, Syntax>,
    ) -> SystemHandler {
        let id = self.system.lock().await.new_system(map);

        SystemHandler {
            id,
            system: self.clone(),
        }
    }

    pub async fn create_actor(&mut self, tx: mpsc::Sender<crate::actor::Message>) -> usize {
        let id = self.system.lock().await.create_actor(tx.clone());
        self.actors.insert(id, tx);

        id
    }

    pub async fn get_actor(&mut self, id: usize) -> Option<mpsc::Sender<crate::actor::Message>> {
        if let Some(tx) = self.actors.get(&id) {
            return Some(tx.clone());
        }

        if let Some(tx) = self.system.lock().await.get_actor(id) {
            self.actors.insert(id, tx.clone());

            Some(tx)
        } else {
            None
        }
    }

    pub async fn drop_actors(&mut self) {
        self.actors.clear();

        self.system.lock().await.drop_actors();
    }
}

#[derive(Clone)]
pub struct SystemHandler {
    id: usize,
    system: SystemHolder,
}

impl SystemHandler {
    pub async fn async_default() -> SystemHandler {
        let system_holder = PrivateSystemHolder::default();
        let system_holder = SystemHolder {
            system: Arc::new(Mutex::new(system_holder)),
            actors: Default::default(),
        };

        SystemHandler {
            id: 0,
            system: system_holder,
        }
    }

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
        self.system
            .get(self.id)
            .await
            .unwrap()
            .do_syscall(ctx, system, no_change, syscall, expr)
            .await
    }

    pub async fn get(&mut self, syscall: SystemCallType) -> Option<Syntax> {
        self.system.get(self.id).await.unwrap().get(syscall).await
    }

    pub fn get_holder(&self) -> SystemHolder {
        self.system.clone()
    }
}
