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
    context::{ContextHandler, ContextHolder},
    errors::InterpreterError,
    lexer::Token,
    parser::{print_ast, Parser, SyntaxKind},
    rational::BigRational,
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
    Contextual(/* ctx_id: */ usize, Box<Syntax>),
    Context(/* ctx_id: */ usize, /* representation: */ String),
    ValAny(),
    ValInt(/* num: */ num::BigInt),
    ValFlt(/* num: */ BigRational),
    ValStr(/* str: */ String),
    ValAtom(/* atom: */ String),
    UnexpectedArguments(),
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
            Self::Contextual(ctx_id, expr) => {
                let expr = expr.reduce().await;

                expr.reduce_contextual(ctx_id).await
            }
            expr @ Self::Context(_, _) => expr,
            Self::UnexpectedArguments() => self,
        }
    }

    async fn reduce_contextual(self, ctx_id: usize) -> Self {
        match self {
            expr @ (Self::ValAny()
            | Self::ValInt(_)
            | Self::ValFlt(_)
            | Self::ValStr(_)
            | Self::ValAtom(_)) => expr,
            Self::Lst(lst) => Self::Lst(
                lst.into_iter()
                    .map(|expr| Self::Contextual(ctx_id, Box::new(expr)))
                    .collect(),
            ),
            Self::Map(map) => Self::Map(
                map.into_iter()
                    .map(|(key, (expr, is_id))| {
                        (key, (Self::Contextual(ctx_id, Box::new(expr)), is_id))
                    })
                    .collect(),
            ),
            Self::BiOp(BiOpType::OpPeriod, lhs, rhs) => Self::BiOp(
                BiOpType::OpPeriod,
                Box::new(Self::Contextual(ctx_id, lhs)),
                rhs,
            ),
            Self::BiOp(op, lhs, rhs) => Self::BiOp(
                op,
                Box::new(Self::Contextual(ctx_id, lhs)),
                Box::new(Self::Contextual(ctx_id, rhs)),
            ),
            Self::Call(lhs, rhs) => Self::Call(
                Box::new(Self::Contextual(ctx_id, lhs)),
                Box::new(Self::Contextual(ctx_id, rhs)),
            ),
            // If a contextual is in a contextual we can use just he inner contextual
            expr @ Self::Contextual(_, _) => expr,
            // Nothing can be done => revert to contextual
            expr @ _ => Self::Contextual(ctx_id, Box::new(expr)),
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
            expr @ Self::Contextual(_, _) => expr,
            expr @ Self::Context(_, _) => expr,
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
            Self::Context(_, _) => {}
            Self::Contextual(_, _) => {}
        }

        result
    }

    #[async_recursion]
    async fn execute_once(
        self,
        first: bool,
        ctx: &mut ContextHandler,
    ) -> Result<Syntax, InterpreterError> {
        let mut values_defined_here = Vec::new();
        let expr: Syntax = match self.clone().reduce().await {
            Self::Id(id) => Ok(
                if let Some(obj) = ctx.get_from_values(&id, &mut values_defined_here).await {
                    obj.clone()
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
            Self::Program(exprs) => Ok(Self::Program(
                join_all(exprs.into_iter().map(|expr| {
                    let mut ctx = ctx.clone();
                    async move { expr.execute(true, &mut ctx).await }
                }))
                .await
                .into_iter()
                .try_collect()?,
            )),
            Self::IfLet(asgs, expr_true, expr_false) => {
                for (lhs, rhs) in asgs {
                    let rhs = rhs.clone().execute(false, ctx).await?;
                    if !ctx
                        .set_values_in_context(
                            &mut ctx.get_holder(),
                            &lhs,
                            &rhs,
                            &mut values_defined_here,
                        )
                        .await
                    {
                        ctx.remove_values(&mut values_defined_here).await;
                        return expr_false.execute_once(false, ctx).await;
                    }
                }

                let mut result = expr_true.clone();
                for (key, value) in ctx
                    .remove_values(&mut values_defined_here)
                    .await
                    .into_iter()
                {
                    result = Box::new(result.replace_args(&key, &value).await);
                }

                Ok(*result)
            }
            Self::Call(box Syntax::Id(id), body) => {
                if let Some(value) = ctx.get_from_values(&id, &mut values_defined_here).await {
                    Ok(Self::Call(Box::new(value), body))
                } else {
                    match (id.as_str(), *body) {
                        ("export", Syntax::Id(id)) => {
                            Ok(if ctx.add_global(&id, &mut values_defined_here).await {
                                Self::ValAny()
                            } else {
                                ctx.push_error(format!("Cannot export symbol {}", id)).await;

                                Self::Call(
                                    Box::new(Syntax::Id(format!("export"))),
                                    Box::new(Syntax::Id(id)),
                                )
                            })
                        }
                        ("import", Syntax::Id(id)) | ("import", Syntax::ValStr(id)) => {
                            import_lib(ctx, id).await
                        }
                        ("syscall", Syntax::Tuple(box Syntax::ValAtom(id), expr))
                            if id == "type" =>
                        {
                            Ok(match *expr {
                                Syntax::ValInt(_) => Syntax::ValAtom("int".to_string()),
                                Syntax::ValFlt(_) => Syntax::ValAtom("float".to_string()),
                                Syntax::ValStr(_) => Syntax::ValAtom("string".to_string()),
                                Syntax::Lst(_) => Syntax::ValAtom("list".to_string()),
                                Syntax::Map(_) => Syntax::ValAtom("map".to_string()),
                                expr @ _ => {
                                    ctx.push_error(format!("Cannot infer type from: {}", expr))
                                        .await;

                                    Syntax::UnexpectedArguments()
                                }
                            })
                        }
                        _ => Ok(self),
                    }
                }
            }
            Self::Call(box Syntax::Lambda(id, fn_expr), expr) => {
                /*insert_into_values(&id, expr.execute(false, ctx)?, ctx);

                let result = fn_expr.execute(false, ctx);
                remove_values(context, &values_defined_here);

                result*/
                Ok(fn_expr.replace_args(&id, &expr).await)
            }
            Self::Call(box Syntax::Contextual(ctx_id, lhs), rhs) => Ok(Self::Contextual(
                ctx_id,
                Box::new(
                    Self::Call(lhs, Box::new(Syntax::Contextual(ctx.get_id(), rhs)))
                        .execute_once(
                            false,
                            &mut ctx
                                .get_holder()
                                .get(ctx_id)
                                .await
                                .unwrap()
                                .handler(ctx.get_holder()),
                        )
                        .await?,
                ),
            )),
            Self::Call(lhs, rhs) => {
                let new_lhs = lhs.clone().execute_once(false, ctx).await?;
                if new_lhs == *lhs {
                    Ok(Self::Call(
                        lhs,
                        Box::new(rhs.execute_once(false, ctx).await?),
                    ))
                } else {
                    Ok(Self::Call(Box::new(new_lhs), rhs))
                }
            }
            Self::Asg(box Self::Id(id), rhs) => {
                if !first {
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

                    let (id, syntax) = extract_id(*lhs, Vec::new()).await?;
                    ctx.insert_fns(
                        id,
                        (
                            unpack_params(syntax, Default::default()).await?,
                            rhs.reduce().await,
                        ),
                    )
                    .await;

                    Ok(Syntax::ValAny())
                }
            }
            Self::Let((lhs, rhs), expr) => {
                let rhs = rhs.execute(false, ctx).await?;
                if !ctx
                    .set_values_in_context(
                        &mut ctx.get_holder(),
                        &lhs,
                        &rhs,
                        &mut values_defined_here,
                    )
                    .await
                {
                    ctx.remove_values(&mut values_defined_here).await;
                    ctx.push_error(format!("Let expression failed: {}", self))
                        .await;

                    Ok(Self::Let((lhs, Box::new(rhs)), expr))
                } else {
                    let mut result = (*expr).clone();
                    for (key, value) in ctx
                        .remove_values(&mut values_defined_here)
                        .await
                        .into_iter()
                    {
                        result = result.replace_args(&key, &value).await;
                    }

                    Ok(result)
                }
            }
            Self::BiOp(
                BiOpType::OpPeriod,
                box Self::Context(ctx_id, ctx_name),
                box Self::Id(id),
            ) => {
                let mut lhs_ctx = ctx.get_holder().get(ctx_id).await.unwrap();

                if let Some(global) = lhs_ctx.get_global(&id).await {
                    Ok(Self::Contextual(ctx_id, Box::new(global)))
                } else {
                    Err(InterpreterError::GlobalNotInContext(ctx_name, id))
                }
            }
            Self::BiOp(BiOpType::OpPeriod, lhs, rhs) => {
                let lhs = lhs.execute_once(false, ctx).await?;
                Ok(Self::BiOp(BiOpType::OpPeriod, Box::new(lhs), rhs))
            }
            Self::BiOp(op @ BiOpType::OpGeq, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpLeq, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpGt, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpLt, lhs, rhs) => {
                let lhs = lhs.execute_once(false, ctx).await?;
                let rhs = rhs.execute_once(false, ctx).await?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Self::BiOp(BiOpType::OpEq, lhs, rhs) => {
                let lhs = lhs.execute(false, ctx).await?;
                let rhs = rhs.execute(false, ctx).await?;

                Ok(if lhs.eval_equal(&rhs).await {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(BiOpType::OpNeq, lhs, rhs) => {
                let lhs = lhs.execute(false, ctx).await?;
                let rhs = rhs.execute(false, ctx).await?;

                Ok(if !lhs.eval_equal(&rhs).await {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(op, lhs, rhs) => {
                let lhs = lhs.execute_once(false, ctx).await?;
                let rhs = rhs.execute_once(false, ctx).await?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Self::If(cond, expr_true, expr_false) => {
                let cond = cond.execute(false, ctx).await?;
                Ok(match cond {
                    Self::ValAtom(id) if id == "true" => expr_true.execute_once(false, ctx).await?,
                    Self::ValAtom(id) if id == "false" => {
                        expr_false.execute_once(false, ctx).await?
                    }
                    _ => {
                        ctx.push_error(format!("Expected :true or :false in if-condition"))
                            .await;
                        self
                    }
                })
            }
            Self::Tuple(lhs, rhs) => {
                let lhs = lhs.execute_once(false, ctx).await?;
                let rhs = rhs.execute_once(false, ctx).await?;

                Ok(Self::Tuple(Box::new(lhs), Box::new(rhs)))
            }
            Self::UnexpectedArguments() => {
                ctx.push_error(format!("Unexpected arguments")).await;
                Ok(self)
            }
            Self::Lst(lst) => {
                let mut result = Vec::new();
                for e in lst {
                    result.push(e.execute_once(false, ctx).await?);
                }

                Ok(Self::Lst(result))
            }
            Self::LstMatch(lst) => {
                let mut result = Vec::new();
                for e in lst {
                    result.push(e.execute_once(false, ctx).await?);
                }

                Ok(Self::LstMatch(result))
            }
            Self::Map(map) => {
                let map =
                    join_all(
                        map.into_iter().map({
                            let ctx = ctx.clone();
                            move |(key, (val, is_id))| {
                                let mut ctx = ctx.clone();

                                async move {
                                    Ok((key, (val.execute_once(false, &mut ctx).await?, is_id)))
                                }
                            }
                        }),
                    )
                    .await
                    .into_iter()
                    .try_collect()?;

                Ok(Self::Map(map))
            }
            Self::MapMatch(_) => Ok(self),
            Self::ExplicitExpr(box expr) => Ok(expr),
            Self::Contextual(ctx_id, expr) if ctx_id == ctx.get_id() => Ok(*expr),
            Self::Contextual(ctx_id, expr) => {
                let holder = ctx.get_holder();
                Ok(Self::Contextual(
                    ctx_id,
                    Box::new(
                        expr.execute_once(
                            first,
                            &mut holder.clone().get(ctx_id).await.unwrap().handler(holder),
                        )
                        .await?,
                    ),
                ))
            }
            expr @ Self::Context(_, _) => Ok(expr),
        }?;

        Ok(expr.reduce().await)
    }

    #[async_recursion]
    pub async fn execute(
        self,
        first: bool,
        ctx: &mut ContextHandler,
    ) -> Result<Syntax, InterpreterError> {
        let mut this = self;
        let mut old = this.clone();
        loop {
            this = this.execute_once(first, ctx).await?;

            if this == old {
                break;
            }

            if first {
                debug!("{}", this);
            }

            old = this.clone();
        }

        Ok(this)
    }

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
async fn import_lib(ctx: &mut ContextHandler, path: String) -> Result<Syntax, InterpreterError> {
    if let Some(ctx) = ctx.get_holder().get_path(&path).await {
        Ok(Syntax::Context(ctx.get_id(), path))
    } else if let Some(ctx_path) = ctx.get_path().await {
        let mut module_path = ctx_path.clone();
        module_path.push(path.clone());

        module_path = std::fs::canonicalize(module_path.clone()).unwrap_or(module_path.clone());

        let module_path_str = module_path.to_string_lossy().to_string();
        if module_path.is_file() {
            if let Some(ctx) = ctx.get_holder().get_path(&module_path_str).await {
                Ok(Syntax::Context(ctx.get_id(), module_path_str))
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
                )
                .await?;

                holder
                    .set_path(&module_path_str, &holder.get(ctx.get_id()).await.unwrap())
                    .await;

                debug!("Imported {} as {}", module_path_str, ctx.get_id());

                Ok(Syntax::Context(ctx.get_id(), module_path_str))
            }
        } else if ["std"].into_iter().any({
            let path = path.clone();
            move |p| p == path
        }) {
            import_std_lib(ctx, path).await
        } else {
            ctx.push_error(format!("Expected {} to be a file", module_path_str))
                .await;

            Err(InterpreterError::ImportFileDoesNotExist(module_path_str))
        }
    } else {
        ctx.push_error("Import can only be done in real modules".to_string())
            .await;

        let result = import_std_lib(ctx, path).await;
        if let Err(InterpreterError::ImportFileDoesNotExist(_)) = result {
            Err(InterpreterError::ContextNotInFile(ctx.get_name().await))
        } else {
            result
        }
    }
}

/// Imports a builtin library
async fn import_std_lib(
    ctx: &mut ContextHandler,
    path: String,
) -> Result<Syntax, InterpreterError> {
    if let Some(ctx) = ctx.get_holder().get_path(&path).await {
        Ok(Syntax::Context(ctx.get_id(), path))
    } else {
        let code = if path == "std" {
            include_str!("./modules/std.pgcl")
        } else {
            ctx.push_error(format!("Expected {} to be a file", path))
                .await;

            return Err(InterpreterError::ImportFileDoesNotExist(path));
        };

        let holder = &mut ctx.get_holder();
        let ctx = execute_code(&path, None, code, holder).await?;
        holder
            .set_path(&path, &holder.get(ctx.get_id()).await.unwrap())
            .await;

        Ok(Syntax::Context(ctx.get_id(), path))
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
async fn execute_code(
    name: &String,
    path: Option<PathBuf>,
    code: &str,
    holder: &mut ContextHolder,
) -> Result<ContextHandler, InterpreterError> {
    let mut ctx = holder
        .create_context(name.clone(), path)
        .await
        .handler(holder.clone());

    let toks: Vec<(SyntaxKind, String)> = Token::lex_for_rowan(code)
        .into_iter()
        .map(
            |(tok, slice)| -> Result<(SyntaxKind, String), InterpreterError> {
                Ok((tok.clone().try_into()?, slice.clone()))
            },
        )
        .try_collect()?;

    debug!("{:?}", toks);

    let typed = parse_to_typed(toks);
    debug!("{:?}", typed);
    let typed = typed?;
    typed.execute(true, &mut ctx).await?;

    Ok(ctx)
}

fn parse_to_typed(toks: Vec<(SyntaxKind, String)>) -> Result<Syntax, InterpreterError> {
    let (ast, errors) = Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
    //print_ast(0, &ast);
    if !errors.is_empty() {
        println!("{:?}", errors);
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
                Self::Contextual(_, expr) => format!("@{}", expr),
                Self::Context(_, id) => format!("{}", id),
            }
            .as_str(),
        )
    }
}
