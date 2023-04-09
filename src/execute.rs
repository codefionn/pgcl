use anyhow::Context;
use async_recursion::async_recursion;
use bigdecimal::BigDecimal;
use futures::future::{join_all, OptionFuture};
use log::debug;
use rowan::GreenNodeBuilder;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};
use tailcall::tailcall;

use crate::{
    actor,
    context::{ContextHandler, ContextHolder, PrivateContext},
    errors::InterpreterError,
    lexer::Token,
    parser::{print_ast, Parser, SyntaxKind},
    rational::BigRational,
    system::{SystemCallType, SystemHandler},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BiOpType {
    /// Operator addition
    OpAdd,
    /// Operator substract
    OpSub,
    /// Operator multiply
    OpMul,
    /// Operator division
    OpDiv,
    /// Operator equal
    OpEq,
    /// Operator not equal
    OpNeq,
    /// Operator strict equals
    OpStrictEq,
    /// Operator not strict equals
    OpStrictNeq,
    /// Operator greater or equals
    OpGeq,
    /// Operator less or equals
    OpLeq,
    /// Operator greater than
    OpGt,
    /// Operator less than
    OpLt,
    /// Operator pipe
    OpPipe,
    /// Operator get in
    OpPeriod,
}

/// Representing a typed syntax tree
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Syntax {
    Program(Vec<Syntax>),
    Lambda(/* id: */ String, /* expr: */ Box<Syntax>),
    Call(/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
    Asg(/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
    Tuple(/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
    Lst(Vec<Syntax>),
    LstMatch(Vec<Syntax>),
    Let(
        /* asg: */ (/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
        /* expr: */ Box<Syntax>,
    ),
    Pipe(Box<Syntax>),
    BiOp(
        /* op: */ BiOpType,
        /* rhs: */ Box<Syntax>,
        /* lhs: */ Box<Syntax>,
    ),
    If(
        /* condition: */ Box<Syntax>,
        /* expr_true: */ Box<Syntax>,
        /* expr_false: */ Box<Syntax>,
    ),
    IfLet(
        /* asgs: */ Vec<(Syntax, Syntax)>,
        /* expr_true: */ Box<Syntax>,
        /* expr_false: */ Box<Syntax>,
    ),
    Id(/* id: */ String),
    Map(
        /* map: */ BTreeMap<String, (Syntax, /* is_id: */ bool)>,
    ),
    MapMatch(
        Vec<(
            String,
            Option<String>,
            Option<Syntax>,
            /* is_id: */ bool,
        )>,
    ),
    ExplicitExpr(Box<Syntax>),
    Contextual(
        /* ctx_id: */ usize,
        /* system_id: */ usize,
        Box<Syntax>,
    ),
    Context(
        /* ctx_id: */ usize,
        /* system_id: */ usize,
        /* representation: */ String,
    ),
    ValAny(),
    ValInt(/* num: */ num::BigInt),
    ValFlt(/* num: */ BigRational),
    ValStr(/* str: */ String),
    ValAtom(/* atom: */ String),
    Signal(SignalType, usize),
    UnexpectedArguments(),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SignalType {
    Actor,
}

impl From<bool> for Syntax {
    fn from(value: bool) -> Self {
        match value {
            true => Self::ValAtom(format!("true")),
            false => Self::ValAtom(format!("false")),
        }
    }
}

#[inline]
fn int_to_flt(x: num::BigInt) -> BigRational {
    x.to_string().parse().unwrap()
}

impl Syntax {
    /// This method reduces the function with very little outside information.
    ///
    /// Should be used for optimizing an expressing before evaluating it.
    #[async_recursion]
    pub async fn reduce(self) -> Self {
        match self {
            Self::Id(_) => self,
            Self::ValAny() => self,
            Self::ValInt(_) => self,
            Self::ValFlt(_) => self,
            Self::ValStr(_) => self,
            Self::ValAtom(_) => self,
            Self::Pipe(_) => self,
            Self::Signal(_, _) => self,
            Self::Program(exprs) => Self::Program(
                join_all(exprs.into_iter().map(|expr| async { expr.reduce().await })).await,
            ),
            Self::Lambda(id, expr) => Self::Lambda(id, Box::new(expr.reduce().await)),
            Self::IfLet(asgs, expr_true, expr_false) => {
                let mut expr_true = expr_true;
                let mut new_asgs = Vec::new();
                for asg in asgs.into_iter() {
                    if let (Self::Id(lhs), Self::Id(rhs)) = asg {
                        expr_true = Box::new(expr_true.replace_args(&lhs, &Self::Id(rhs)).await);
                    } else {
                        new_asgs.push(asg);
                    }
                }

                if new_asgs.is_empty() {
                    expr_true.reduce().await
                } else {
                    Self::IfLet(
                        new_asgs,
                        Box::new(expr_true.reduce().await),
                        Box::new(expr_false.reduce().await),
                    )
                }
            }
            Self::BiOp(BiOpType::OpPipe, lhs, rhs) => Self::Call(rhs, lhs),
            Self::BiOp(
                BiOpType::OpAdd,
                box Self::ValInt(x),
                box Self::BiOp(BiOpType::OpAdd, box Self::ValInt(y), expr),
            ) => Self::BiOp(BiOpType::OpAdd, Box::new(Self::ValInt(x + y)), expr),
            Self::BiOp(
                BiOpType::OpSub,
                box Self::ValInt(x),
                box Self::BiOp(BiOpType::OpSub, box Self::ValInt(y), expr),
            ) => Self::BiOp(BiOpType::OpAdd, Box::new(Self::ValInt(x - y)), expr),
            Self::BiOp(
                BiOpType::OpMul,
                box Self::ValInt(x),
                box Self::BiOp(BiOpType::OpMul, box Self::ValInt(y), expr),
            ) => Self::BiOp(BiOpType::OpMul, Box::new(Self::ValInt(x * y)), expr),
            Self::BiOp(
                BiOpType::OpDiv,
                box Self::ValInt(x),
                box Self::BiOp(BiOpType::OpDiv, box Self::ValInt(y), expr),
            ) => Self::BiOp(
                BiOpType::OpMul,
                Box::new(Self::ValFlt(int_to_flt(x) / int_to_flt(y))),
                expr,
            ),
            Self::BiOp(BiOpType::OpAdd, box Self::ValInt(x), box Self::ValInt(y)) => {
                Self::ValInt(x + y)
            }
            Self::BiOp(BiOpType::OpSub, box Self::ValInt(x), box Self::ValInt(y)) => {
                Self::ValInt(x - y)
            }
            Self::BiOp(BiOpType::OpMul, box Self::ValInt(x), box Self::ValInt(y)) => {
                Self::ValInt(x * y)
            }
            Self::BiOp(BiOpType::OpDiv, box Self::ValInt(x), box Self::ValInt(y)) => {
                Self::ValFlt(int_to_flt(x) / int_to_flt(y))
            }
            Self::BiOp(BiOpType::OpAdd, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(x + y)
            }
            Self::BiOp(BiOpType::OpSub, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(x - y)
            }
            Self::BiOp(BiOpType::OpMul, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(x * y)
            }
            Self::BiOp(BiOpType::OpDiv, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(x / y)
            }
            Self::BiOp(BiOpType::OpAdd, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Self::ValFlt(x + int_to_flt(y))
            }
            Self::BiOp(BiOpType::OpSub, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Self::ValFlt(x - int_to_flt(y))
            }
            Self::BiOp(BiOpType::OpMul, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Self::ValFlt(x * int_to_flt(y))
            }
            Self::BiOp(BiOpType::OpDiv, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Self::ValFlt(x / int_to_flt(y))
            }
            Self::BiOp(BiOpType::OpAdd, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(int_to_flt(x) + y)
            }
            Self::BiOp(BiOpType::OpSub, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(int_to_flt(x) - y)
            }
            Self::BiOp(BiOpType::OpMul, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(int_to_flt(x) * y)
            }
            Self::BiOp(BiOpType::OpDiv, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Self::ValFlt(int_to_flt(x) / y)
            }
            Self::BiOp(BiOpType::OpAdd, box Self::ValStr(x), box Self::ValStr(y)) => {
                Self::ValStr(x + y.as_str())
            }
            Self::BiOp(BiOpType::OpAdd, box Self::Lst(lhs), box Self::Lst(rhs)) => {
                let mut lst = Vec::new();
                lst.extend(lhs.into_iter());
                lst.extend(rhs.into_iter());

                Self::Lst(lst)
            }
            Self::BiOp(
                BiOpType::OpAdd,
                box Self::Lst(lhs),
                box Self::BiOp(BiOpType::OpAdd, box Self::Lst(rhs), rest),
            ) => {
                let mut lst = Vec::new();
                lst.extend(lhs.into_iter());
                lst.extend(rhs.into_iter());

                Self::BiOp(BiOpType::OpAdd, Box::new(Self::Lst(lst)), rest)
            }
            Self::BiOp(BiOpType::OpAdd, box Self::Map(lhs), box Self::Map(rhs)) => {
                let mut map = BTreeMap::new();
                // Keys from lhs overwrite keys from rhs
                map.extend(rhs.into_iter());
                map.extend(lhs.into_iter());

                Self::Map(map)
            }
            Self::BiOp(
                BiOpType::OpAdd,
                box Self::Map(lhs),
                box Self::BiOp(BiOpType::OpAdd, box Self::Map(rhs), rest),
            ) => {
                let mut map = BTreeMap::new();
                // Keys from lhs overwrite keys from rhs
                map.extend(rhs.into_iter());
                map.extend(lhs.into_iter());

                Self::BiOp(BiOpType::OpAdd, Box::new(Self::Map(map)), rest)
            }
            Self::BiOp(BiOpType::OpGeq, box Self::ValInt(x), box Self::ValInt(y)) => {
                (x >= y).into()
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValInt(x), box Self::ValInt(y)) => {
                (x <= y).into()
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValInt(x), box Self::ValInt(y)) => (x > y).into(),
            Self::BiOp(BiOpType::OpLt, box Self::ValInt(x), box Self::ValInt(y)) => (x < y).into(),
            Self::BiOp(BiOpType::OpGeq, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                (x >= y).into()
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                (x <= y).into()
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValFlt(x), box Self::ValFlt(y)) => (x > y).into(),
            Self::BiOp(BiOpType::OpLt, box Self::ValFlt(x), box Self::ValFlt(y)) => (x < y).into(),
            Self::BiOp(BiOpType::OpGeq, box Self::ValInt(x), box Self::ValFlt(y)) => {
                (int_to_flt(x) >= y).into()
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValInt(x), box Self::ValFlt(y)) => {
                (int_to_flt(x) <= y).into()
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValInt(x), box Self::ValFlt(y)) => {
                (int_to_flt(x) > y).into()
            }
            Self::BiOp(BiOpType::OpLt, box Self::ValInt(x), box Self::ValFlt(y)) => {
                (int_to_flt(x) < y).into()
            }
            Self::BiOp(BiOpType::OpGeq, box Self::ValFlt(x), box Self::ValInt(y)) => {
                (x >= int_to_flt(y)).into()
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValFlt(x), box Self::ValInt(y)) => {
                (x <= int_to_flt(y)).into()
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValFlt(x), box Self::ValInt(y)) => {
                (x > int_to_flt(y)).into()
            }
            Self::BiOp(BiOpType::OpLt, box Self::ValFlt(x), box Self::ValInt(y)) => {
                (x < int_to_flt(y)).into()
            }
            Self::BiOp(BiOpType::OpEq, box Self::ValAny(), _) => Self::ValAtom("true".to_string()),
            Self::BiOp(BiOpType::OpEq, _, box Self::ValAny()) => Self::ValAtom("true".to_string()),
            Self::BiOp(BiOpType::OpNeq, box Self::ValAny(), _) => {
                Self::ValAtom("false".to_string())
            }
            Self::BiOp(BiOpType::OpNeq, _, box Self::ValAny()) => {
                Self::ValAtom("false".to_string())
            }
            Self::Call(box Self::Lambda(id, box Self::Id(id_in_expr)), expr)
                if id == id_in_expr =>
            {
                *expr
            }
            Self::Tuple(a, b) => {
                Self::Tuple(Box::new(a.reduce().await), Box::new(b.reduce().await))
            }
            Self::Call(lhs, rhs) => {
                Self::Call(Box::new(lhs.reduce().await), Box::new(rhs.reduce().await))
            }
            Self::Let((box Self::Id(id), rhs), expr) => expr.replace_args(&id, &rhs).await,
            Self::Let(_, _) => self,
            Self::Asg(lhs, rhs) => Self::Asg(lhs, Box::new(rhs.reduce().await)),
            Self::If(cond, lhs, rhs) => Self::If(
                Box::new(cond.reduce().await),
                Box::new(lhs.reduce().await),
                Box::new(rhs.reduce().await),
            ),
            Self::BiOp(op, a, b) => {
                Self::BiOp(op, Box::new(a.reduce().await), Box::new(b.reduce().await))
            }
            Self::Lst(lst) => {
                Self::Lst(join_all(lst.into_iter().map(|expr| async { expr.reduce().await })).await)
            }
            Self::LstMatch(lst) => {
                Self::LstMatch(join_all(lst.into_iter().map(|expr| expr.reduce())).await)
            }
            Self::Map(map) => {
                let map =
                    join_all(map.into_iter().map(|(key, (val, is_id))| async move {
                        (key, (val.reduce().await, is_id))
                    }))
                    .await
                    .into_iter()
                    .collect();

                Self::Map(map)
            }
            Self::MapMatch(map) => {
                let map = join_all(
                    map.into_iter()
                        .map(|(key, into_key, val, is_id)| async move {
                            let val: OptionFuture<_> =
                                val.map(|expr| async { expr.reduce().await }).into();
                            (key, into_key, val.await, is_id)
                        }),
                )
                .await;
                Self::MapMatch(map)
            }
            Self::ExplicitExpr(expr) => expr.reduce().await,
            Self::Contextual(ctx_id, system_id, expr) => {
                let expr = expr.reduce().await;

                expr.reduce_contextual(ctx_id, system_id).await
            }
            expr @ Self::Context(_, _, _) => expr,
            Self::UnexpectedArguments() => self,
        }
    }

    async fn reduce_contextual(self, ctx_id: usize, system_id: usize) -> Self {
        match self {
            expr @ (Self::ValAny()
            | Self::ValInt(_)
            | Self::ValFlt(_)
            | Self::ValStr(_)
            | Self::ValAtom(_)
            | Self::Signal(_, _)) => expr,
            Self::Lst(lst) => Self::Lst(
                join_all(lst.into_iter().map(|expr| async move {
                    Self::Contextual(ctx_id, system_id, Box::new(expr))
                        .reduce()
                        .await
                }))
                .await,
            ),
            Self::Map(map) => Self::Map(
                join_all(map.into_iter().map(|(key, (expr, is_id))| async move {
                    (
                        key,
                        (
                            Self::Contextual(ctx_id, system_id, Box::new(expr))
                                .reduce()
                                .await,
                            is_id,
                        ),
                    )
                }))
                .await
                .into_iter()
                .collect(),
            ),
            Self::BiOp(BiOpType::OpPeriod, lhs, rhs) => Self::BiOp(
                BiOpType::OpPeriod,
                Box::new(Self::Contextual(ctx_id, system_id, lhs).reduce().await),
                Box::new(rhs.reduce().await),
            ),
            Self::BiOp(op, lhs, rhs) => Self::BiOp(
                op,
                Box::new(Self::Contextual(ctx_id, system_id, lhs).reduce().await),
                Box::new(Self::Contextual(ctx_id, system_id, rhs).reduce().await),
            ),
            Self::Call(lhs, rhs) => Self::Call(
                Box::new(Self::Contextual(ctx_id, system_id, lhs).reduce().await),
                Box::new(Self::Contextual(ctx_id, system_id, rhs).reduce().await),
            ),
            Self::Tuple(lhs, rhs) => Self::Tuple(
                Box::new(Self::Contextual(ctx_id, system_id, lhs).reduce().await),
                Box::new(Self::Contextual(ctx_id, system_id, rhs).reduce().await),
            ),
            expr @ Self::Context(_, _, _) => expr,
            // If a contextual is in a contextual we can use just he inner contextual
            expr @ Self::Contextual(_, _, _) => expr,
            // Nothing can be done => revert to contextual
            expr @ _ => Self::Contextual(ctx_id, system_id, Box::new(expr)),
        }
    }

    #[async_recursion]
    async fn replace_args(self, key: &String, value: &Syntax) -> Syntax {
        match self {
            expr @ Self::Program(_) => expr,
            Self::Id(id) if *id == *key => value.clone(),
            Self::Id(_) => self,
            Self::Lambda(id, expr) if *id != *key => {
                Self::Lambda(id, Box::new(expr.replace_args(key, value).await))
            }
            Self::Lambda(_, _) => self,
            Self::Call(lhs, rhs) => {
                let lhs = lhs.replace_args(key, value).await;
                let rhs = rhs.replace_args(key, value).await;
                Self::Call(Box::new(lhs), Box::new(rhs))
            }
            Self::Asg(lhs, rhs) => {
                let lhs = lhs.replace_args(key, value).await;
                let rhs = rhs.replace_args(key, value).await;
                Self::Asg(Box::new(lhs), Box::new(rhs))
            }
            Self::Tuple(lhs, rhs) => {
                let lhs = lhs.replace_args(key, value).await;
                let rhs = rhs.replace_args(key, value).await;
                Self::Tuple(Box::new(lhs), Box::new(rhs))
            }
            Self::Let((lhs, rhs), expr) => {
                let rhs = rhs.replace_args(key, value);
                if lhs.get_args().await.contains(key) {
                    Self::Let((lhs, Box::new(rhs.await)), expr)
                } else {
                    let expr = expr.replace_args(key, value);
                    Self::Let((lhs, Box::new(rhs.await)), Box::new(expr.await))
                }
            }
            Self::BiOp(BiOpType::OpPeriod, lhs, rhs) => {
                let lhs = lhs.replace_args(key, value);
                Self::BiOp(BiOpType::OpPeriod, Box::new(lhs.await), rhs)
            }
            Self::BiOp(op, lhs, rhs) => {
                let lhs = lhs.replace_args(key, value);
                let rhs = rhs.replace_args(key, value);
                Self::BiOp(op, Box::new(lhs.await), Box::new(rhs.await))
            }
            Self::If(cond, expr_true, expr_false) => {
                let cond = cond.replace_args(key, value);
                let expr_true = expr_true.replace_args(key, value);
                let expr_false = expr_false.replace_args(key, value);
                Self::If(
                    Box::new(cond.await),
                    Box::new(expr_true.await),
                    Box::new(expr_false.await),
                )
            }
            Self::IfLet(asgs, expr_true, expr_false) => {
                let asgs: Vec<(Syntax, Syntax)> = join_all(
                    asgs.into_iter()
                        .map(|(lhs, rhs)| async { (lhs, rhs.replace_args(key, value).await) }),
                )
                .await;

                let expr_false = expr_false.replace_args(key, value).await;
                if join_all(asgs.iter().map(|(_, rhs)| rhs.get_args()))
                    .await
                    .into_iter()
                    .flatten()
                    .collect::<HashSet<String>>()
                    .contains(key)
                {
                    Self::IfLet(asgs, expr_true, Box::new(expr_false))
                } else {
                    let expr_true = expr_true.replace_args(key, value);
                    Self::IfLet(asgs, Box::new(expr_true.await), Box::new(expr_false))
                }
            }
            Self::UnexpectedArguments() => self,
            Self::ValAny() => self,
            Self::ValInt(_) => self,
            Self::ValFlt(_) => self,
            Self::ValStr(_) => self,
            Self::ValAtom(_) => self,
            Self::Lst(lst) => Self::Lst(
                join_all(
                    lst.into_iter()
                        .map(|expr| async { expr.replace_args(key, value).await }),
                )
                .await,
            ),
            Self::LstMatch(lst) => Self::LstMatch(
                join_all(
                    lst.into_iter()
                        .map(|expr| async { expr.replace_args(key, value).await }),
                )
                .await,
            ),
            Self::Map(map) => {
                let map = join_all(
                    map.into_iter()
                        .map(|(map_key, (map_val, is_id))| async move {
                            (map_key, (map_val.replace_args(&key, value).await, is_id))
                        }),
                )
                .await;

                Self::Map(map.into_iter().collect())
            }
            Self::MapMatch(map) => {
                let map = join_all(map.into_iter().map(
                    |(map_key, map_into_key, map_val, is_id)| async move {
                        let map_val: OptionFuture<_> = map_val
                            .map(|x| async { x.replace_args(&key, value).await })
                            .into();

                        (map_key, map_into_key, map_val.await, is_id)
                    },
                ))
                .await;

                Self::MapMatch(map)
            }
            Self::ExplicitExpr(expr) => {
                Self::ExplicitExpr(Box::new(expr.replace_args(key, value).await))
            }
            expr @ Self::Contextual(_, _, _) => expr,
            expr @ Self::Context(_, _, _) => expr,
            expr @ Self::Signal(_, _) => expr,
            Self::Pipe(expr) => Self::Pipe(Box::new(expr.replace_args(key, value).await)),
        }
    }

    #[async_recursion]
    async fn get_args(&self) -> HashSet<String> {
        let mut result = HashSet::new();
        match self {
            Self::Id(id) => {
                result.insert(id.clone());
            }
            Self::Program(_) => {}
            Self::Lambda(_, expr) => {
                result.extend(expr.get_args().await);
            }
            Self::Call(lhs, rhs)
            | Self::Asg(lhs, rhs)
            | Self::Tuple(lhs, rhs)
            | Self::BiOp(_, lhs, rhs) => {
                result.extend(lhs.get_args().await);
                result.extend(rhs.get_args().await);
            }
            Self::Let((lhs, rhs), expr) => {
                result.extend(lhs.get_args().await);
                result.extend(rhs.get_args().await);
                result.extend(expr.get_args().await);
            }
            Self::If(cond, expr_true, expr_false) => {
                result.extend(cond.get_args().await);
                result.extend(expr_true.get_args().await);
                result.extend(expr_false.get_args().await);
            }
            Self::IfLet(asgs, expr_true, expr_false) => {
                result.extend(
                    join_all(asgs.into_iter().map(|(lhs, rhs)| async {
                        let mut result = lhs.get_args().await;
                        result.extend(rhs.get_args().await);

                        result
                    }))
                    .await
                    .into_iter()
                    .flatten(),
                );

                result.extend(expr_true.get_args().await);
                result.extend(expr_false.get_args().await);
            }
            Self::UnexpectedArguments()
            | Self::ValAny()
            | Self::ValInt(_)
            | Self::ValFlt(_)
            | Self::ValStr(_)
            | Self::ValAtom(_) => {}
            Self::Lst(lst) => {
                result.extend(
                    join_all(lst.into_iter().map(|expr| async { expr.get_args().await }))
                        .await
                        .into_iter()
                        .flatten(),
                );
            }
            Self::LstMatch(lst) => {
                result.extend(
                    join_all(lst.into_iter().map(|expr| async { expr.get_args().await }))
                        .await
                        .into_iter()
                        .flatten(),
                );
            }
            Self::Map(map) => {
                result.extend(
                    map.iter()
                        .filter(|(_, (_, is_id))| *is_id)
                        .map(|(id, _)| id.clone()),
                );
            }
            Self::MapMatch(map) => {
                result.extend(map.iter().map(|(key, into_key, _, _)| {
                    if let Some(into_key) = into_key {
                        into_key.clone()
                    } else {
                        key.clone()
                    }
                }));
            }
            Self::ExplicitExpr(expr) => {
                result.extend(expr.get_args().await);
            }
            Self::Pipe(expr) => result.extend(expr.get_args().await),
            Self::Context(_, _, _) => {}
            Self::Contextual(_, _, _) => {}
            Self::Signal(_, _) => {}
        }

        result
    }

    /// Executes the AST once (tries to do minimal changes)
    ///
    /// ## Arguments
    ///
    /// - `first`: Is a top-level expression
    /// - `no_change`: There's wasn't any change since the last call of `execute_once` (Maybe
    /// change a little more?)
    /// - `ctx`: The current context
    /// - `system`: The current system
    #[async_recursion]
    pub async fn execute_once(
        self,
        first: bool,
        no_change: bool,
        ctx: &mut ContextHandler,
        system: &mut SystemHandler,
    ) -> Result<Syntax, InterpreterError> {
        // This local context helps by not locking up the current context `ctx`
        let mut local_ctx = PrivateContext::new(usize::MAX, "local".to_string(), None);

        let mut values_defined_here = Vec::new();
        let expr: Syntax = match self.clone().reduce().await {
            Self::Id(id) => Ok(
                if let Some(obj) = ctx.get_from_values(&id, &mut values_defined_here).await {
                    if first {
                        if let Ok(obj) = obj.clone().execute(false, ctx, system).await {
                            ctx.replace_from_values(&id, obj.clone()).await;

                            obj
                        } else {
                            obj.clone()
                        }
                    } else {
                        obj.clone()
                    }
                } else {
                    self
                },
            ),
            Self::ValAny() => Ok(self),
            Self::ValInt(_) => Ok(self),
            Self::ValFlt(_) => Ok(self),
            Self::ValStr(_) => Ok(self),
            Self::ValAtom(_) => Ok(self),
            Self::Lambda(_, _) => Ok(self),
            Self::Pipe(_) => Ok(self),
            Self::Program(exprs) => {
                let mut exprs: Vec<_> = join_all(exprs.into_iter().map(|expr| {
                    let mut ctx = ctx.clone();
                    let mut system = system.clone();

                    async move { expr.execute(first, &mut ctx, &mut system).await }
                }))
                .await
                .into_iter()
                .try_collect()?;

                // If the program has at least one expression => return the result of the last one
                if let Some(expr) = exprs.pop() {
                    Ok(expr)
                } else {
                    Ok(Self::ValAny())
                }
            }
            Self::IfLet(asgs, expr_true, expr_false) => {
                for (lhs, rhs) in asgs {
                    let rhs = rhs.clone().execute(false, ctx, system).await?;
                    if !local_ctx
                        .set_values_in_context(
                            &mut ctx.get_holder(),
                            &lhs,
                            &rhs,
                            &mut values_defined_here,
                        )
                        .await
                    {
                        local_ctx.remove_values(&mut values_defined_here);
                        return Ok(*expr_false);
                    }
                }

                let mut result = expr_true.clone();
                for (key, value) in local_ctx
                    .remove_values(&mut values_defined_here)
                    .into_iter()
                {
                    result = Box::new(result.replace_args(&key, &value).await);
                }

                Ok(*result)
            }
            Self::Call(box Syntax::Signal(signal_type, signal_id), expr) => match signal_type {
                SignalType::Actor => {
                    if let Some(tx) = system.get_holder().get_actor(signal_id).await {
                        if let Err(_) = tx.send(actor::Message::Signal(*expr)).await {
                            Ok(Syntax::ValAtom("false".to_string()))
                        } else {
                            Ok(Syntax::ValAtom("true".to_string()))
                        }
                    } else {
                        Ok(Syntax::ValAtom("false".to_string()))
                    }
                }
                _ => Ok(Syntax::ValAtom("false".to_string())),
            },
            Self::Call(box Syntax::Id(id), body) => {
                make_call(
                    ctx,
                    system,
                    no_change,
                    id,
                    body,
                    self,
                    &mut values_defined_here,
                )
                .await
            }
            Self::Call(box Syntax::Lambda(id, fn_expr), expr) => {
                /*insert_into_values(&id, expr.execute(false, ctx)?, ctx);

                let result = fn_expr.execute(false, ctx);
                remove_values(context, &values_defined_here);

                result*/
                Ok(fn_expr.replace_args(&id, &expr).await)
            }
            Self::Call(box Syntax::Contextual(ctx_id, system_id, box Syntax::Id(id)), rhs) => {
                let mut ctx = ctx.get_holder().get_handler(ctx_id).await.unwrap();
                let mut system = system.get_holder().get_handler(system_id).await.unwrap();

                Ok(Self::Contextual(
                    ctx_id,
                    system_id,
                    Box::new(
                        make_call(
                            &mut ctx,
                            &mut system,
                            no_change,
                            id,
                            rhs,
                            self.clone(),
                            &mut values_defined_here,
                        )
                        .await?,
                    ),
                ))
            }
            Self::Call(box Syntax::Contextual(ctx_id, system_id, lhs), rhs) => {
                let old_ctx_id = ctx.get_id();
                let old_system_id = system.get_id();

                let mut ctx = ctx.get_holder().get_handler(ctx_id).await.unwrap();
                let mut system = system.get_holder().get_handler(system_id).await.unwrap();

                // Create a new contextual with the contents evaulated in the given context
                // The contextual is reduced away if possible in the next execution step
                Ok(Self::Contextual(
                    ctx_id,
                    system_id,
                    Box::new(
                        Self::Call(
                            lhs,
                            Box::new(Syntax::Contextual(old_ctx_id, old_system_id, rhs)),
                        )
                        .execute_once(false, no_change, &mut ctx, &mut system)
                        .await?,
                    ),
                ))
            }
            Self::Call(lhs, rhs) => {
                let old_lhs = lhs.clone();
                let lhs = lhs.execute_once(false, no_change, ctx, system).await?;
                if lhs == *old_lhs {
                    // Executing the left side doesn't seem to do the trick anymore
                    // => evaulate the right side (this can produces more desirable results)
                    Ok(Self::Call(
                        Box::new(lhs),
                        Box::new(rhs.execute_once(false, no_change, ctx, system).await?),
                    ))
                } else {
                    Ok(Self::Call(Box::new(lhs), rhs))
                }
            }
            Self::Asg(box Self::Id(id), rhs) => {
                if !first {
                    // Assignments are only allowed on the top-level
                    Ok(self)
                } else {
                    ctx.insert_into_values(&id, *rhs, &mut values_defined_here)
                        .await;
                    Ok(Self::ValAny())
                }
            }
            Self::Asg(lhs, rhs) => {
                if !first {
                    Ok(self)
                } else {
                    #[tailcall]
                    async fn extract_id(
                        syntax: Syntax,
                        mut unpacked: Vec<Syntax>,
                    ) -> Result<(String, Syntax), InterpreterError> {
                        match syntax {
                            Syntax::Call(box Syntax::Id(id), rhs) => {
                                unpacked.push(*rhs);

                                fn rebuild(syntax: Syntax, unpacked: &[Syntax]) -> Syntax {
                                    if unpacked.is_empty() {
                                        syntax
                                    } else {
                                        Syntax::Call(
                                            Box::new(rebuild(
                                                unpacked[unpacked.len() - 1].clone(),
                                                &unpacked[..unpacked.len() - 1],
                                            )),
                                            Box::new(syntax),
                                        )
                                    }
                                }

                                Ok((
                                    id,
                                    rebuild(
                                        unpacked[unpacked.len() - 1].clone(),
                                        &unpacked[..unpacked.len() - 1],
                                    ),
                                ))
                            }
                            Syntax::Call(lhs, rhs) => {
                                unpacked.push(*rhs.clone());
                                extract_id(*lhs.clone(), unpacked)
                            }
                            _ => Err(InterpreterError::ExpectedCall()),
                        }
                    }

                    #[async_recursion]
                    async fn unpack_params(
                        syntax: Syntax,
                        mut result: Vec<Syntax>,
                    ) -> Result<Vec<Syntax>, InterpreterError> {
                        match syntax {
                            Syntax::Call(a, b) => {
                                result.insert(0, *b);
                                unpack_params(*a, result).await
                            }
                            _ => {
                                result.insert(0, syntax);
                                Ok(result)
                            }
                        }
                    }

                    if let Ok((id, syntax)) = extract_id(*lhs.clone(), Vec::new()).await {
                        ctx.insert_fns(
                            id,
                            (
                                unpack_params(syntax, Default::default()).await?,
                                rhs.reduce().await,
                            ),
                        )
                        .await
                    } else {
                        // This left-hand side isn't prepended by an identifier
                        // => Maybe it's a normal assignment of a let expression?
                        let rhs = rhs.execute(false, ctx, system).await?;
                        if !ctx
                            .clone()
                            .set_values_in_context(
                                &mut ctx.get_holder(),
                                &lhs.clone(),
                                &rhs,
                                &mut values_defined_here,
                            )
                            .await
                        {
                            ctx.remove_values(&mut values_defined_here).await;
                            ctx.push_error(format!(
                                "{} must be assignable to {} but is not",
                                lhs, rhs
                            ))
                            .await;
                        } else {
                            values_defined_here.clear();
                        }
                    }

                    Ok(Syntax::ValAny())
                }
            }
            Self::Let((lhs, rhs), expr) => {
                if no_change {
                    // no changes happen in the RHS of the assignment
                    // => evaulate the let expression
                    if !local_ctx
                        .set_values_in_context(
                            &mut ctx.get_holder(),
                            &lhs,
                            &rhs,
                            &mut values_defined_here,
                        )
                        .await
                    {
                        local_ctx.remove_values(&mut values_defined_here);
                        ctx.push_error(format!("Let expression failed: {}", self))
                            .await;

                        Ok(Self::Let((lhs, rhs), expr))
                    } else {
                        let mut result = (*expr).clone();
                        for (key, value) in local_ctx
                            .remove_values(&mut values_defined_here)
                            .into_iter()
                        {
                            result = result.replace_args(&key, &value).await;
                        }

                        Ok(result)
                    }
                } else {
                    Ok(Self::Let(
                        (
                            lhs,
                            Box::new(rhs.execute_once(first, no_change, ctx, system).await?),
                        ),
                        expr,
                    ))
                }
            }
            Self::BiOp(BiOpType::OpPeriod, box Self::Map(mut map), box Self::Id(id)) => {
                if let Some((expr, _)) = map.remove(&id) {
                    Ok(expr)
                } else {
                    Ok(self)
                }
            }
            Self::BiOp(
                BiOpType::OpPeriod,
                box Self::Context(ctx_id, system_id, ctx_name),
                box Self::Id(id),
            ) => {
                let mut lhs_ctx = ctx.get_holder().get(ctx_id).await.unwrap();

                if let Some(global) = lhs_ctx.get_global(&id).await {
                    Ok(Self::Contextual(ctx_id, system_id, Box::new(global)))
                } else {
                    Err(InterpreterError::GlobalNotInContext(ctx_name, id))
                }
            }
            Self::BiOp(BiOpType::OpPeriod, lhs, rhs) => {
                let lhs = lhs.execute_once(false, no_change, ctx, system).await?;
                Ok(Self::BiOp(BiOpType::OpPeriod, Box::new(lhs), rhs))
            }
            Self::BiOp(op @ BiOpType::OpGeq, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpLeq, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpGt, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpLt, lhs, rhs) => {
                let lhs = lhs.execute_once(false, no_change, ctx, system).await?;
                let rhs = rhs.execute_once(false, no_change, ctx, system).await?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Self::BiOp(op @ BiOpType::OpEq, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpNeq, lhs, rhs) => {
                if no_change {
                    let old_lhs = lhs.clone();
                    let old_rhs = rhs.clone();
                    let lhs = lhs.execute_once(false, true, ctx, system).await?;
                    let rhs = rhs.execute_once(false, true, ctx, system).await?;

                    if lhs == *old_lhs && rhs == *old_rhs {
                        if op == BiOpType::OpEq {
                            Ok(if lhs.eval_equal(&rhs).await {
                                Self::ValAtom("true".to_string())
                            } else {
                                Self::ValAtom("false".to_string())
                            })
                        } else {
                            Ok(if !lhs.eval_equal(&rhs).await {
                                Self::ValAtom("true".to_string())
                            } else {
                                Self::ValAtom("false".to_string())
                            })
                        }
                    } else {
                        Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
                    }
                } else {
                    Ok(Self::BiOp(
                        op,
                        Box::new(lhs.execute_once(false, no_change, ctx, system).await?),
                        Box::new(rhs.execute_once(false, no_change, ctx, system).await?),
                    ))
                }
            }
            Self::BiOp(op, lhs, rhs) => {
                let lhs = lhs.execute_once(false, no_change, ctx, system).await?;
                let rhs = rhs.execute_once(false, no_change, ctx, system).await?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Self::If(cond, expr_true, expr_false) => {
                if no_change {
                    // the condition doesn't change anymore
                    // => try to evaluate the condition
                    Ok(match *cond {
                        Self::ValAtom(id) if id == "true" => *expr_true,
                        Self::ValAtom(id) if id == "false" => *expr_false,
                        _ => {
                            // That failed => try to evaluate the expression even more
                            let old_cond = cond.clone();
                            let cond = cond.execute_once(false, true, ctx, system).await?;

                            if cond == *old_cond {
                                ctx.push_error(format!("Expected :true or :false in if-condition"))
                                    .await;
                                self
                            } else {
                                Self::If(Box::new(cond), expr_true, expr_false)
                            }
                        }
                    })
                } else {
                    Ok(Self::If(
                        Box::new(cond.execute_once(false, false, ctx, system).await?),
                        expr_true,
                        expr_false,
                    ))
                }
            }
            Self::Tuple(lhs, rhs) => {
                let lhs = lhs.execute_once(false, no_change, ctx, system).await?;
                let rhs = rhs.execute_once(false, no_change, ctx, system).await?;

                Ok(Self::Tuple(Box::new(lhs), Box::new(rhs)))
            }
            Self::UnexpectedArguments() => {
                ctx.push_error(format!("Unexpected arguments")).await;
                Ok(self)
            }
            Self::Lst(lst) => {
                let mut result = Vec::with_capacity(lst.len());
                for e in lst {
                    result.push(e.execute_once(false, no_change, ctx, system).await?);
                }

                Ok(Self::Lst(result))
            }
            Self::LstMatch(lst) => {
                let mut result = Vec::new();
                for e in lst {
                    result.push(e.execute_once(false, no_change, ctx, system).await?);
                }

                Ok(Self::LstMatch(result))
            }
            Self::Map(map) => {
                let map = join_all(map.into_iter().map({
                    let ctx = ctx.clone();
                    move |(key, (val, is_id))| {
                        let mut ctx = ctx.clone();
                        let mut system = system.clone();

                        async move {
                            Ok((
                                key,
                                (
                                    val.execute_once(false, no_change, &mut ctx, &mut system)
                                        .await?,
                                    is_id,
                                ),
                            ))
                        }
                    }
                }))
                .await
                .into_iter()
                .try_collect()?;

                Ok(Self::Map(map))
            }
            Self::MapMatch(_) => Ok(self),
            Self::ExplicitExpr(box expr) => Ok(expr),
            Self::Contextual(ctx_id, system_id, expr)
                if ctx_id == ctx.get_id() && system_id == system.get_id() =>
            {
                Ok(*expr)
            }
            Self::Contextual(ctx_id, system_id, expr) => {
                let holder = ctx.get_holder();
                Ok(Self::Contextual(
                    ctx_id,
                    system_id,
                    Box::new(
                        expr.execute_once(
                            first,
                            no_change,
                            &mut holder.clone().get_handler(ctx_id).await.unwrap(),
                            system,
                        )
                        .await?,
                    ),
                ))
            }
            expr @ Self::Context(_, _, _) => Ok(expr),
            expr @ Self::Signal(_, _) => Ok(expr),
        }?;

        Ok(expr)
    }

    /// Execute the AST until no longer possible
    #[async_recursion]
    pub async fn execute(
        self,
        first: bool,
        ctx: &mut ContextHandler,
        system: &mut SystemHandler,
    ) -> Result<Syntax, InterpreterError> {
        let mut this = self;
        let mut old = this.clone();
        let mut haschanged = true;
        loop {
            this = this
                .reduce()
                .await
                .execute_once(first, !haschanged, ctx, system)
                .await?;

            // Execute as long there are changes
            if this == old {
                // This is necessary, because some expressions are only evaluated if there aren't
                // any changes happening (like the condition of an if expression)
                if haschanged {
                    haschanged = false;
                } else {
                    break;
                }
            } else {
                haschanged = true;
            }

            if first && this != Syntax::ValAny() && haschanged {
                debug!("{}", this);
            }

            old = this.clone();
        }

        Ok(this)
    }

    /// Evaluate, if both statements are equal with type-coercion
    pub async fn eval_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Syntax::Tuple(a0, b0), Syntax::Tuple(a1, b1)) => a0 == a1 && b0 == b1,
            (Syntax::Call(a0, b0), Syntax::Call(a1, b1)) => a0 == a1 && b0 == b1,
            (Syntax::BiOp(BiOpType::OpPipe, a0, b0), Syntax::BiOp(BiOpType::OpPipe, a1, b1)) => {
                a0 == a1 && b0 == b1
            }
            (Syntax::BiOp(BiOpType::OpPipe, b0, a0), Syntax::Call(a1, b1)) => a0 == a1 && b0 == b1,
            (Syntax::Call(a0, b0), Syntax::BiOp(BiOpType::OpPipe, b1, a1)) => a0 == a1 && b0 == b1,
            (Syntax::ValAny(), _) => true,
            (_, Syntax::ValAny()) => true,
            (Syntax::ValAtom(a), Syntax::ValAtom(b)) => a == b,
            (Syntax::ValInt(a), Syntax::ValInt(b)) => a == b,
            (Syntax::ValFlt(a), Syntax::ValFlt(b)) => a == b,
            (Syntax::ValFlt(a), Syntax::ValInt(b)) => *a == *b,
            (Syntax::ValInt(b), Syntax::ValFlt(a)) => *a == *b,
            (Syntax::ValStr(a), Syntax::ValStr(b)) => a == b,
            _ => false,
        }
    }
}

/// Imports the specified library (including builtin libraries)
async fn import_lib(
    ctx: &mut ContextHandler,
    system: &mut SystemHandler,
    path: String,
    builtins_map: BTreeMap<String, Syntax>,
) -> Result<Syntax, InterpreterError> {
    let has_restrict = builtins_map.get("restrict") == Some(&Syntax::ValAtom("true".to_string()));
    debug!("has_restrict: {}", has_restrict);

    // List all systemcalls here
    let mut builtins_map: BTreeMap<String, Syntax> = builtins_map
        .into_iter()
        .filter(|(key, _)| {
            SystemCallType::all()
                .iter()
                .map(|syscall| syscall.to_systemcall())
                .any(move |can_be| can_be == key)
        })
        .collect();

    let old_ctx_id = ctx.get_id();
    let old_system_id = system.get_id();

    let mut system = if builtins_map.is_empty() && !has_restrict {
        system.clone()
    } else {
        let mut new_builtins_map = BTreeMap::<SystemCallType, Syntax>::new();
        let all_syscalls = [SystemCallType::Typeof];
        for syscall_type in all_syscalls {
            let syscall_name = syscall_type.to_systemcall();

            if let Some((_, expr)) = builtins_map.remove_entry(syscall_name) {
                new_builtins_map.insert(
                    syscall_type,
                    Syntax::Contextual(old_ctx_id, old_system_id, Box::new(expr)),
                );
            } else if let Some(expr) = system.get(syscall_type).await {
                new_builtins_map.insert(syscall_type, expr);
            }
        }

        if has_restrict {
            for syscall_type in all_syscalls {
                if !new_builtins_map.contains_key(&syscall_type) {
                    debug!("{:?}", syscall_type);
                    new_builtins_map.insert(syscall_type, Syntax::ValAtom("error".to_string()));
                }
            }
        }

        system
            .get_holder()
            .new_system_handler(new_builtins_map)
            .await
    };

    if let Some(ctx) = ctx.get_holder().get_path(&path).await {
        Ok(Syntax::Context(ctx.get_id(), system.get_id(), path))
    } else if let Some(ctx_path) = ctx.get_path().await {
        let mut module_path = ctx_path.clone();
        module_path.push(path.clone());

        module_path = std::fs::canonicalize(module_path.clone()).unwrap_or(module_path.clone());

        let module_path_str = module_path.to_string_lossy().to_string();
        if module_path.is_file() {
            if let Some(ctx) = ctx.get_holder().get_path(&module_path_str).await {
                Ok(Syntax::Context(
                    ctx.get_id(),
                    system.get_id(),
                    module_path_str,
                ))
            } else {
                let code = std::fs::read_to_string(&module_path).map_err({
                    let module_path_str = module_path_str.clone();
                    move |_| InterpreterError::ImportFileDoesNotExist(module_path_str)
                })?;

                let holder = &mut ctx.get_holder();

                let ctx = execute_code(
                    &module_path_str,
                    module_path.parent().clone().map(|path| path.to_path_buf()),
                    code.as_str(),
                    holder,
                    &mut system,
                )
                .await?;

                debug!("Imported {} as {}", module_path_str, ctx.get_id());

                Ok(Syntax::Context(
                    ctx.get_id(),
                    system.get_id(),
                    module_path_str,
                ))
            }
        } else if
        /* List all builtin libraries here -> */
        ["std", "sys", "str"].into_iter().any({
            let path = path.clone();
            move |p| p == path
        }) {
            import_std_lib(ctx, &mut system, path).await
        } else {
            ctx.push_error(format!("Expected {} to be a file", module_path_str))
                .await;

            Err(InterpreterError::ImportFileDoesNotExist(module_path_str))
        }
    } else {
        ctx.push_error("Import can only be done in real modules".to_string())
            .await;

        let result = import_std_lib(ctx, &mut system, path).await;
        if let Err(InterpreterError::ImportFileDoesNotExist(_)) = result {
            Err(InterpreterError::ContextNotInFile(ctx.get_name().await))
        } else {
            result
        }
    }
}

#[async_recursion]
async fn make_call(
    ctx: &mut ContextHandler,
    system: &mut SystemHandler,
    no_change: bool,
    id: String,
    body: Box<Syntax>,
    original_expr: Syntax,
    values_defined_here: &mut Vec<String>,
) -> Result<Syntax, InterpreterError> {
    if let Some(value) = ctx.get_from_values(&id, values_defined_here).await {
        Ok(Syntax::Call(Box::new(value), body))
    } else {
        match (id.as_str(), body.reduce().await) {
            ("export", Syntax::Id(id)) => {
                if let Some(val) = ctx.get_from_values(&id, values_defined_here).await {
                    if let Ok(val) = val.execute(false, ctx, system).await {
                        ctx.replace_from_values(&id, val).await;
                    }

                    if ctx.add_global(&id, values_defined_here).await {
                        return Ok(Syntax::ValAny());
                    }
                }

                ctx.push_error(format!("Cannot export symbol {}", id)).await;

                return Ok(Syntax::Call(
                    Box::new(Syntax::Id(format!("export"))),
                    Box::new(Syntax::Id(id)),
                ));
            }
            ("import", Syntax::Id(id)) | ("import", Syntax::ValStr(id)) => {
                import_lib(ctx, system, id, Default::default()).await
            }
            ("import", Syntax::Contextual(ctx_id, system_id, body @ box Syntax::Id(_)))
            | ("import", Syntax::Contextual(ctx_id, system_id, body @ box Syntax::ValStr(_))) => {
                let mut ctx = ctx.get_holder().get_handler(ctx_id).await.unwrap();
                let mut system = system.get_holder().get_handler(system_id).await.unwrap();

                make_call(
                    &mut ctx,
                    &mut system,
                    no_change,
                    "import".to_string(),
                    body,
                    original_expr,
                    values_defined_here,
                )
                .await
            }
            ("import", Syntax::Tuple(box Syntax::Id(id), box Syntax::Map(map)))
            | ("import", Syntax::Tuple(box Syntax::ValStr(id), box Syntax::Map(map))) => {
                import_lib(
                    ctx,
                    system,
                    id,
                    map.into_iter()
                        .filter(|(_, (_, is_id))| *is_id)
                        .map(|(key, (value, _))| (key, value))
                        .collect(),
                )
                .await
            }
            (
                "import",
                Syntax::Contextual(
                    ctx_id,
                    system_id,
                    body @ box Syntax::Tuple(box Syntax::Id(_), box Syntax::Map(_)),
                ),
            )
            | (
                "import",
                Syntax::Contextual(
                    ctx_id,
                    system_id,
                    body @ box Syntax::Tuple(box Syntax::ValStr(_), box Syntax::Map(_)),
                ),
            ) => {
                let mut ctx = ctx.get_holder().get_handler(ctx_id).await.unwrap();
                let mut system = system.get_holder().get_handler(system_id).await.unwrap();

                make_call(
                    &mut ctx,
                    &mut system,
                    no_change,
                    "import".to_string(),
                    body,
                    original_expr,
                    values_defined_here,
                )
                .await
            }
            ("syscall", Syntax::Tuple(box Syntax::ValAtom(id), expr)) => match id.as_str() {
                "type" => {
                    system
                        .clone()
                        .do_syscall(ctx, system, no_change, SystemCallType::Typeof, *expr)
                        .await
                }
                "time" => {
                    system
                        .clone()
                        .do_syscall(ctx, system, no_change, SystemCallType::MeasureTime, *expr)
                        .await
                }
                "cmd" => {
                    system
                        .clone()
                        .do_syscall(ctx, system, no_change, SystemCallType::Cmd, *expr)
                        .await
                }
                "println" => {
                    system
                        .clone()
                        .do_syscall(ctx, system, no_change, SystemCallType::Println, *expr)
                        .await
                }
                "actor" => {
                    system
                        .clone()
                        .do_syscall(ctx, system, no_change, SystemCallType::Actor, *expr)
                        .await
                }
                "exitactor" => {
                    system
                        .clone()
                        .do_syscall(ctx, system, no_change, SystemCallType::ExitActor, *expr)
                        .await
                }
                "httprequest" => {
                    system
                        .clone()
                        .do_syscall(ctx, system, no_change, SystemCallType::HttpRequest, *expr)
                        .await
                }
                _ => Ok(original_expr),
            },
            _ => Ok(original_expr),
        }
    }
}

/// Imports a builtin library
async fn import_std_lib(
    ctx: &mut ContextHandler,
    system: &mut SystemHandler,
    path: String,
) -> Result<Syntax, InterpreterError> {
    if let Some(ctx) = ctx.get_holder().get_path(&path).await {
        Ok(Syntax::Context(ctx.get_id(), system.get_id(), path))
    } else {
        let code = if path == "std" {
            include_str!("./modules/std.pgcl")
        } else if path == "sys" {
            include_str!("./modules/sys.pgcl")
        } else if path == "str" {
            include_str!("./modules/str.pgcl")
        } else {
            ctx.push_error(format!("Expected {} to be a file", path))
                .await;

            return Err(InterpreterError::ImportFileDoesNotExist(path));
        };

        let holder = &mut ctx.get_holder();
        let ctx = execute_code(&path, None, code, holder, system).await?;

        Ok(Syntax::Context(ctx.get_id(), system.get_id(), path))
    }
}

/// Executes code (a module)
///
/// ## Parameters
///
/// - `name`: Name of the code-module
/// - `path`: The real path to the module
/// - `code`: The code itself
/// - `holder`: The context holder
pub async fn execute_code(
    name: &String,
    path: Option<PathBuf>,
    code: &str,
    holder: &mut ContextHolder,
    system: &mut SystemHandler,
) -> Result<ContextHandler, InterpreterError> {
    let mut ctx = holder
        .create_context(name.clone(), path)
        .await
        .handler(holder.clone());
    holder
        .set_path(name, &holder.get(ctx.get_id()).await.unwrap())
        .await;

    let toks: Vec<(SyntaxKind, String)> = Token::lex_for_rowan(code)
        .into_iter()
        .map(
            |(tok, slice)| -> Result<(SyntaxKind, String), InterpreterError> {
                Ok((tok.clone().try_into()?, slice.clone()))
            },
        )
        .try_collect()?;

    //debug!("{:?}", toks);

    let typed = parse_to_typed(toks);
    debug!("{:?}", typed);
    let typed = typed?;
    typed.execute(true, &mut ctx, system).await?;

    Ok(ctx)
}

fn parse_to_typed(toks: Vec<(SyntaxKind, String)>) -> Result<Syntax, InterpreterError> {
    let (ast, errors) = Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
    //print_ast(0, &ast);
    if !errors.is_empty() {
        eprintln!("{:?}", errors);
    }

    return (*ast).try_into();
}

impl std::fmt::Display for BiOpType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::OpPeriod => ".",
            Self::OpAdd => "+",
            Self::OpSub => "-",
            Self::OpMul => "*",
            Self::OpDiv => "/",
            Self::OpEq => "==",
            Self::OpNeq => "!=",
            Self::OpStrictEq => "===",
            Self::OpStrictNeq => "!==",
            Self::OpGeq => ">=",
            Self::OpLeq => "<=",
            Self::OpGt => ">",
            Self::OpLt => "<",
            Self::OpPipe => "|",
        })
    }
}

impl std::fmt::Display for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn val_str(x: &String) -> String {
            format!(
                "\"{}\"",
                x.chars()
                    .into_iter()
                    .map(|c| match c {
                        '\\' => format!("\\"),
                        '\n' => format!("\\n"),
                        '\r' => format!("\\r"),
                        '\t' => format!("\\t"),
                        '\0' => format!("\\0"),
                        '\"' => format!("\\\""),
                        c => format!("{}", c),
                    })
                    .collect::<String>()
            )
        }

        f.write_str(
            match self {
                Self::Program(exprs) => {
                    exprs
                        .iter()
                        .map(|expr| format!("{}", expr))
                        .fold(String::new(), |a, b| {
                            if a.is_empty() {
                                b
                            } else {
                                format!("{}\n{}", a, b)
                            }
                        })
                }
                Self::Lambda(id, expr) => format!("(\\{} {})", id, expr.to_string()),
                Self::Call(lhs, rhs) => format!("({} {})", lhs, rhs),
                Self::Asg(lhs, rhs) => format!("({} = {})", lhs.to_string(), rhs.to_string()),
                Self::Tuple(lhs, rhs) => format!("({}, {})", lhs.to_string(), rhs.to_string()),
                Self::Let((lhs, rhs), expr) => format!("(let {} = {} in {})", lhs, rhs, expr),
                Self::Pipe(expr) => format!("| {}", expr),
                Self::BiOp(BiOpType::OpPeriod, lhs, rhs) => format!("({}.{})", lhs, rhs),
                Self::BiOp(op, lhs, rhs) => format!("({} {} {})", lhs, op, rhs),
                Self::IfLet(asgs, expr_true, expr_false) => format!(
                    "if let {} then {} else {}",
                    asgs.iter()
                        .map(|(lhs, rhs)| format!("{} = {}", lhs, rhs))
                        .fold(String::new(), |x, y| if x.is_empty() {
                            format!("{}", y)
                        } else {
                            format!("{}; {}", x, y)
                        }),
                    expr_true,
                    expr_false
                ),
                Self::If(cond, lhs, rhs) => format!("if {} then {} else {}", cond, lhs, rhs),
                Self::Id(id) => format!("{}", id),
                Self::UnexpectedArguments() => format!("UnexpectedArguments"),
                Self::ValAny() => format!("_"),
                Self::ValInt(x) => x.to_string(),
                Self::ValFlt(x) => {
                    let x: BigDecimal = x.clone().into();
                    format!("{}", x)
                }
                .trim_end_matches(|c| c == '0' || c == '.')
                .to_string(),
                Self::ValStr(x) => val_str(x),
                Self::ValAtom(x) => format!("@{}", x),
                Self::Lst(lst) => format!(
                    "[{}]",
                    lst.iter()
                        .map(|x| format!("{}", x))
                        .fold(String::new(), |x, y| {
                            if x.is_empty() {
                                format!("{}", y)
                            } else {
                                format!("{}, {}", x, y)
                            }
                        })
                ),
                Self::LstMatch(lst) => format!(
                    "[{}]",
                    lst.iter()
                        .map(|x| format!("{}", x))
                        .fold(String::new(), |x, y| {
                            if x.is_empty() {
                                format!("{}", y)
                            } else {
                                format!("{};{}", x, y)
                            }
                        })
                ),
                Self::Map(map) => {
                    if map.is_empty() {
                        format!("{{}}")
                    } else {
                        format!(
                            "{{ {} }}",
                            map.iter()
                                .map(|(key, (val, is_id))| format!(
                                    "{}: {}",
                                    if *is_id { key.clone() } else { val_str(key) },
                                    val
                                ))
                                .fold(String::new(), |x, y| {
                                    if x.is_empty() {
                                        y
                                    } else {
                                        format!("{}, {}", x, y)
                                    }
                                })
                        )
                    }
                }
                Self::MapMatch(map) => {
                    if map.is_empty() {
                        format!("{{}}")
                    } else {
                        format!(
                            "{{ {} }}",
                            map.iter()
                                .map(|(key, key_into, val, is_id)| {
                                    let key = if let Some(key_into) = key_into {
                                        format!("{} {}", val_str(key), key_into)
                                    } else {
                                        format!(
                                            "{}",
                                            if *is_id { key.clone() } else { val_str(key) }
                                        )
                                    };

                                    if let Some(val) = val {
                                        format!("{}: {}", key, val)
                                    } else {
                                        key
                                    }
                                })
                                .fold(String::new(), |x, y| {
                                    if x.is_empty() {
                                        y
                                    } else {
                                        format!("{}, {}", x, y)
                                    }
                                })
                        )
                    }
                }
                Self::ExplicitExpr(expr) => format!("{}", expr),
                Self::Contextual(_, _, expr) => format!("@{}", expr),
                Self::Context(_, _, id) => format!("{}", id),
                Self::Signal(_, _) => format!("signal"),
            }
            .as_str(),
        )
    }
}
