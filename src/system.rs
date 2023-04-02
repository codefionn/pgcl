use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use num::FromPrimitive;
use tokio::{sync::Mutex, time::Instant};

use crate::{
    context::ContextHandler, errors::InterpreterError, execute::Syntax, rational::BigRational,
};

#[derive(Clone, Debug, PartialEq, Hash, Eq, PartialOrd, Ord, Copy)]
pub enum SystemCallType {
    Typeof,
    MeasureTime,
}

impl SystemCallType {
    pub fn to_systemcall(&self) -> &'static str {
        match self {
            Self::Typeof => "type",
            Self::MeasureTime => "time",
        }
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
}

#[derive(Clone)]
pub struct SystemHolder {
    system: Arc<Mutex<PrivateSystemHolder>>,
}

impl Default for SystemHolder {
    fn default() -> Self {
        SystemHolder {
            system: Arc::new(Mutex::new(PrivateSystemHolder::default())),
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
