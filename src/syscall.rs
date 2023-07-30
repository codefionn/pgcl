use std::collections::BTreeMap;

use crate::{
    actor,
    context::ContextHandler,
    errors::InterpreterError,
    execute::{Executor, SignalType, Syntax},
    rational::BigRational,
    system::SystemHandler, runner::Runner,
};
use bigdecimal::{BigDecimal, ToPrimitive};
use futures::SinkExt;
use log::{debug, error};
use num::FromPrimitive;
use tokio::{process::Command, sync::oneshot, time::Instant};

#[derive(Clone, Debug, PartialEq, Hash, Eq, PartialOrd, Ord, Copy)]
pub enum SystemCallType {
    Typeof,
    MeasureTime,
    Cmd,
    Println,
    Actor,
    ExitActor,
    HttpRequest,
    ExitThisProgram,
    CreateMsg,
    RecvMsg,
}

impl SystemCallType {
    pub const fn to_systemcall(self) -> &'static str {
        match self {
            Self::Typeof => "type",
            Self::MeasureTime => "time",
            Self::Cmd => "cmd",
            Self::Println => "println",
            Self::Actor => "actor",
            Self::ExitActor => "exitactor",
            Self::HttpRequest => "httprequest",
            Self::ExitThisProgram => "exit",
            Self::CreateMsg => "createmsg",
            Self::RecvMsg => "recvmsg",
        }
    }

    pub const fn all() -> &'static [SystemCallType] {
        &[
            Self::Typeof,
            Self::MeasureTime,
            Self::Cmd,
            Self::Println,
            Self::Actor,
            Self::HttpRequest,
            Self::ExitActor,
            Self::ExitThisProgram,
            Self::CreateMsg,
            Self::RecvMsg,
        ]
    }

    pub const fn is_secure(&self) -> bool {
        match self {
            Self::Typeof
            | Self::MeasureTime
            | Self::Cmd
            | Self::Println
            | Self::Actor
            | Self::ExitActor
            | Self::CreateMsg
            | Self::RecvMsg => true,
            _ => false,
        }
    }
}

impl TryFrom<&str> for SystemCallType {
    type Error = InterpreterError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let all_syscalls = Self::all();
        for syscall in all_syscalls {
            if syscall.to_systemcall() == value {
                return Ok(*syscall);
            }
        }

        Err(InterpreterError::UnknownError())
    }
}

pub struct PrivateSystem {
    id: usize,
    map: BTreeMap<SystemCallType, Syntax>,
}

impl PrivateSystem {
    pub fn new(id: usize, map: BTreeMap<SystemCallType, Syntax>) -> Self {
        Self { id, map }
    }

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
        if let Some(expr) = self.map.get(&syscall) {
            return Ok(expr.clone());
        }

        match (syscall, expr) {
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

                Ok(match expr {
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
                        ctx.push_error(format!("Cannot infer type from: {expr}"))
                            .await;

                        Syntax::UnexpectedArguments()
                    }
                })
            }
            (SystemCallType::MeasureTime, expr) => {
                let now = Instant::now();

                let expr = Executor::new(ctx, system, runner, show_steps)
                    .execute(expr, false)
                    .await?;
                std::mem::drop(runner);

                let diff = now.elapsed().as_secs_f64();
                let diff: BigRational = BigRational::from_f64(diff).unwrap();

                Ok(Syntax::Tuple(
                    Box::new(Syntax::ValFlt(diff)),
                    Box::new(expr),
                ))
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

                Ok(Syntax::Map(map.into_iter().collect()))
            })
            .unwrap_or(Ok(Syntax::ValAtom("error".to_string()))),
            (SystemCallType::Println, Syntax::ValStr(s)) => {
                println!("{s}");

                Ok(Syntax::ValAny())
            }
            (SystemCallType::Println, Syntax::ValInt(i)) => {
                println!("{i}");

                Ok(Syntax::ValAny())
            }
            (SystemCallType::Println, Syntax::ValFlt(f)) => {
                let f: BigDecimal = f.into();
                println!("{f}");

                Ok(Syntax::ValAny())
            }
            (SystemCallType::Actor, Syntax::Tuple(init, actor_fn)) => {
                let (handle, tx) =
                    crate::actor::create_actor(ctx.clone(), system.clone(), *init, *actor_fn).await;
                let id = system.create_actor(handle, tx).await;

                Ok(Syntax::Signal(SignalType::Actor, id))
            }
            (SystemCallType::ExitActor, Syntax::Signal(signal_type, signal_id)) => {
                match signal_type {
                    SignalType::Actor => match system.get_actor(signal_id).await {
                        Some(tx) => {
                            let (tx_exit, rx_exit) = oneshot::channel();
                            match tx.send(actor::Message::Exit(tx_exit)).await {
                                Ok(_) => {
                                    if let Err(err) = rx_exit.await {
                                        error!("{}", err);
                                    }

                                    Ok(Syntax::ValAtom("true".to_string()))
                                }
                                Err(_) => Ok(Syntax::ValAtom("false".to_string())),
                            }
                        }
                        _ => Ok(Syntax::ValAtom("false".to_string())),
                    },
                    _ => Ok(false.into()),
                }
            }
            (
                SystemCallType::HttpRequest,
                Syntax::Tuple(box Syntax::ValStr(uri), box Syntax::Map(mut settings)),
            ) => {
                let method = settings
                    .remove("method")
                    .and_then(|(method, _)| match method {
                        Syntax::ValAtom(method) => match method.as_str() {
                            "GET" => Some(hyper::Method::GET),
                            "POST" => Some(hyper::Method::POST),
                            "PUT" => Some(hyper::Method::PUT),
                            "DELETE" => Some(hyper::Method::DELETE),
                            "HEAD" => Some(hyper::Method::HEAD),
                            "OPTIONS" => Some(hyper::Method::OPTIONS),
                            "CONNECT" => Some(hyper::Method::CONNECT),
                            "PATCH" => Some(hyper::Method::PATCH),
                            "TRACE" => Some(hyper::Method::TRACE),
                            _ => None,
                        },
                        _ => None,
                    })
                    .unwrap_or(hyper::Method::GET);

                let body = settings.remove("method").and_then(|(body, _)| match body {
                    Syntax::ValStr(body) => Some(body.into_bytes()),
                    _ => None,
                });

                let headers: BTreeMap<String, String> = settings
                    .remove("headers")
                    .and_then(|(headers, _)| match headers {
                        Syntax::Map(headers) => Some(
                            headers
                                .into_iter()
                                .map(|(key, (val, _))| {
                                    (
                                        key,
                                        match val {
                                            Syntax::ValStr(val) => Some(val),
                                            _ => None,
                                        },
                                    )
                                })
                                .filter(|(_, val)| val.is_some())
                                .map(|(key, val)| (key, val.unwrap()))
                                .collect(),
                        ),
                        _ => None,
                    })
                    .unwrap_or(BTreeMap::new());

                debug!("Trying HTTP Request for \"{}\"", uri);
                if let Ok(uri) = uri.parse::<hyper::Uri>() {
                    Ok(match uri.scheme_str() {
                        Some("http") => {
                            let client = hyper::Client::builder().build_http();
                            do_http_request(uri, method, headers, body, client).await?
                        }
                        Some("https") => {
                            let https = hyper_tls::HttpsConnector::new();
                            let client = hyper::Client::builder().build(https);
                            do_http_request(uri, method, headers, body, client).await?
                        }
                        Some(_) | None => {
                            debug!("Unsupported scheme: {:?}", uri.scheme_str());

                            false.into()
                        }
                    })
                } else {
                    Ok(Syntax::ValAtom("false".to_string()))
                }
            }
            (SystemCallType::ExitThisProgram, Syntax::ValInt(id)) => Err(
                InterpreterError::ProgramTerminatedByUser(id.to_i32().unwrap_or(1)),
            ),
            (SystemCallType::CreateMsg, Syntax::ValAny()) => {
                let handle = system.create_message().await;
                Ok(Syntax::Signal(SignalType::Message, handle))
            }
            (SystemCallType::RecvMsg, Syntax::Signal(SignalType::Message, id)) => system
                .recv_message(id)
                .await
                .map_err(|err| InterpreterError::InternalError(format!("{}", err))),
            (syscall, expr) => Ok(Syntax::Call(
                Box::new(Syntax::Id("syscall".to_string())),
                Box::new(Syntax::Tuple(
                    Box::new(Syntax::ValAtom(syscall.to_systemcall().to_string())),
                    Box::new(
                        Executor::new(ctx, system, runner, show_steps)
                            .execute_once(expr, false, no_change)
                            .await?,
                    ),
                )),
            )),
        }
    }

    pub async fn get(&self, syscall: SystemCallType) -> Option<Syntax> {
        self.map.get(&syscall).cloned()
    }
}

async fn do_http_request<Connector>(
    uri: hyper::Uri,
    method: hyper::Method,
    headers: BTreeMap<String, String>,
    body: Option<Vec<u8>>,
    client: hyper::Client<Connector, hyper::Body>,
) -> Result<Syntax, InterpreterError>
where
    Connector: hyper::client::connect::Connect + Clone + std::marker::Send + Sync + 'static,
{
    let mut req = hyper::Request::builder().method(method).uri(uri.clone());
    for (key, val) in headers.into_iter() {
        req = req.header(key, val);
    }

    let req: hyper::Request<hyper::Body> = if let Some(body) = body {
        if let Ok(req) = req.body(hyper::Body::from(body)) {
            req
        } else {
            debug!("Failed due to body");
            return Ok(Syntax::ValAtom("false".to_string()));
        }
    } else if let Ok(req) = req.body(hyper::Body::empty()) {
        req
    } else {
        debug!("Failed due to body");
        return Ok(Syntax::ValAtom("false".to_string()));
    };

    match client.request(req).await {
        Ok(resp) => {
            let mut result: BTreeMap<&str, Syntax> = BTreeMap::new();
            result.insert("ok", resp.status().is_success().into());
            result.insert("status", Syntax::ValInt(resp.status().as_u16().into()));
            let body = hyper::body::to_bytes(resp.into_body())
                .await
                .ok()
                .and_then(|bytes| String::from_utf8(bytes.into_iter().collect()).ok());
            if let Some(body) = body {
                result.insert("body", Syntax::ValStr(body));
            } else {
                result.insert("body", false.into());
            }

            Ok(Syntax::Map(
                result
                    .into_iter()
                    .map(|(key, val)| (key.to_string(), (val, true)))
                    .collect(),
            ))
        }
        Err(err) => {
            debug!(
                "HTTP Request to \"{}\" failed due to {}",
                uri.to_string(),
                err
            );

            Ok(Syntax::ValAtom("false".to_string()))
        }
    }
}
