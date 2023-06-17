use async_recursion::async_recursion;
use bigdecimal::{BigDecimal, ToPrimitive};
use futures::{
    future::{join_all, OptionFuture},
    StreamExt,
};
use std::collections::{BTreeMap, HashSet};
use num::pow::Pow;
use num::Zero;
use num::FromPrimitive;

use crate::rational::BigRational;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BiOpType {
    /// Operator power
    OpPow,
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
    Message,
}

impl From<bool> for Syntax {
    fn from(value: bool) -> Self {
        match value {
            true => Self::ValAtom("true".to_string()),
            false => Self::ValAtom("false".to_string()),
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
                futures::stream::iter(
                    exprs
                        .into_iter()
                        .map(|expr| async { expr.reduce_all().await }),
                )
                .buffered(4)
                .collect()
                .await,
            ),
            Self::Lambda(id, expr) => Self::Lambda(id, Box::new(expr.reduce().await)),
            Self::IfLet(asgs, expr_true, expr_false) => {
                let mut expr_true = expr_true;
                let mut new_asgs = Vec::new();
                for asg in asgs.into_iter() {
                    match asg {
                        (Self::Id(lhs), Self::Id(rhs)) => {
                            expr_true =
                                Box::new(expr_true.replace_args(&lhs, &Self::Id(rhs)).await);
                        }
                        (Self::ValInt(x), Self::ValInt(y)) => {
                            if x != y {
                                return *expr_false;
                            }
                        }
                        (Self::ValFlt(x), Self::ValFlt(y)) => {
                            if x != y {
                                return *expr_false;
                            }
                        }
                        (Self::ValAtom(x), Self::ValAtom(y)) => {
                            if x != y {
                                return *expr_false;
                            }
                        }
                        (Self::ValStr(x), Self::ValStr(y)) => {
                            if x != y {
                                return *expr_false;
                            }
                        }
                        _ => {
                            new_asgs.push(asg);
                        }
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
                box Self::BiOp(BiOpType::OpAdd, box Self::ValInt(y), expr),
            ) => Self::BiOp(BiOpType::OpSub, Box::new(Self::ValInt(x - y)), expr),
            Self::BiOp(
                BiOpType::OpSub,
                box Self::ValInt(x),
                box Self::BiOp(BiOpType::OpSub, box Self::ValInt(y), expr),
            ) => Self::BiOp(BiOpType::OpAdd, Box::new(Self::ValInt(x - y)), expr),
            Self::BiOp(
                BiOpType::OpAdd,
                box Self::BiOp(BiOpType::OpAdd, expr, box Self::ValInt(x)),
                box Self::ValInt(y),
            ) => Self::BiOp(BiOpType::OpAdd, expr, Box::new(Self::ValInt(x + y))),
            Self::BiOp(
                BiOpType::OpSub,
                box Self::BiOp(BiOpType::OpSub, expr, box Self::ValInt(x)),
                box Self::ValInt(y),
            ) => Self::BiOp(BiOpType::OpAdd, expr, Box::new(Self::ValInt(-x - y))),
            Self::BiOp(
                BiOpType::OpAdd,
                box Self::BiOp(BiOpType::OpSub, expr, box Self::ValInt(x)),
                box Self::ValInt(y),
            ) => Self::BiOp(BiOpType::OpAdd, expr, Box::new(Self::ValInt(-x + y))),
            Self::BiOp(
                BiOpType::OpSub,
                box Self::BiOp(BiOpType::OpAdd, expr, box Self::ValInt(x)),
                box Self::ValInt(y),
            ) => Self::BiOp(BiOpType::OpAdd, expr, Box::new(Self::ValInt(x - y))),
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
            Self::BiOp(BiOpType::OpPow, box Self::ValInt(x), box Self::ValInt(y)) => {
                if y == 0.into() {
                    Self::ValInt(1.into())
                } else if y > 0.into() && y <= u64::MAX.into() {
                    let y: u64 = y.try_into().unwrap();
                    Self::ValInt(x.pow(y))
                } else if y < 0.into() && -y.clone() <= u64::MAX.into() {
                    let y: u64 = (-y).try_into().unwrap();
                    Self::ValFlt(BigRational::new(1, x.pow(y)))
                } else {
                    Self::UnexpectedArguments()
                }
            },
            Self::BiOp(BiOpType::OpPow, box Self::ValInt(x), box Self::ValFlt(y)) => {
                if y.is_zero() {
                    Self::ValInt(1.into())
                } else {
                    let yp = y.clone().abs();
                    let (u, d) = yp.clone().split();
                    let ints = u.clone() / d.clone();
                    let roots_parts = u - ints.clone() * d.clone();

                    let ints: u64 = match ints.try_into() {
                        Ok(i) => i,
                        Err(_) => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValInt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    let mut result = BigRational::new(x.clone().pow(ints), 1);

                    let roots_parts: u64 = match roots_parts.try_into() {
                        Ok(i) => i,
                        Err(_) => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValInt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    let d: u32 = match d.try_into() {
                        Ok(d) => d,
                        Err(_) => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValInt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    let xf: f64 = match x.to_u64().map(|xf| xf as f64) {
                        Some(xf) => xf,
                        _ => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValInt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    println!("{}", roots_parts);
                    for _ in 0..roots_parts {
                        println!("{:?}, {}, {}", result, d, xf.clone().sqrt());
                        if d == 2 {
                            result = result * BigRational::from_f64(xf.clone().sqrt()).unwrap();
                        } else {
                            result = result * BigRational::from_f64(xf.clone().powf(1.0 / d as f64)).unwrap();
                        }
                    }

                    if y < BigRational::new(0, 1) {
                        result = BigRational::new(-1, 1) / result;
                    }

                    Self::ValFlt(result)
                }
            }
            Self::BiOp(BiOpType::OpPow, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                if y.is_zero() {
                    Self::ValInt(1.into())
                } else {
                    let yp = y.clone().abs();
                    let (u, d) = yp.clone().split();
                    let ints = u.clone() / d.clone();
                    let roots_parts = u - ints.clone() * d.clone();

                    let ints: i32 = match ints.try_into() {
                        Ok(i) => i,
                        Err(_) => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValFlt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    let xf: f64 = match x.clone().try_into() {
                        Ok(xf) => xf,
                        _ => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValFlt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    let mut result = BigRational::from_f64(xf.clone().powi(ints)).unwrap();

                    let roots_parts: u64 = match roots_parts.try_into() {
                        Ok(i) => i,
                        Err(_) => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValFlt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    let d: u32 = match d.try_into() {
                        Ok(d) => d,
                        Err(_) => {
                            return Self::BiOp(BiOpType::OpPow, Box::new(Self::ValFlt(x)), Box::new(Self::ValFlt(y)));
                        }
                    };

                    println!("{}", roots_parts);
                    for _ in 0..roots_parts {
                        println!("{:?}, {}, {}", result, d, xf.clone().sqrt());
                        if d == 2 {
                            result = result * BigRational::from_f64(xf.clone().sqrt()).unwrap();
                        } else {
                            result = result * BigRational::from_f64(xf.clone().powf(1.0 / d as f64)).unwrap();
                        }
                    }

                    if y < BigRational::new(0, 1) {
                        result = BigRational::new(1, 1) / result;
                    }

                    Self::ValFlt(result)
                }
            }
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
            Self::Lst(lst) => Self::Lst(
                futures::stream::iter(lst.into_iter().map(|expr| async { expr.reduce().await }))
                    .buffered(4)
                    .collect()
                    .await,
            ),
            Self::LstMatch(lst) => Self::LstMatch(
                futures::stream::iter(lst.into_iter().map(|expr| expr.reduce()))
                    .buffered(4)
                    .collect()
                    .await,
            ),
            Self::Map(map) => {
                let map =
                    futures::stream::iter(map.into_iter().map(|(key, (val, is_id))| async move {
                        (key, (val.reduce().await, is_id))
                    }))
                    .buffer_unordered(4)
                    .collect()
                    .await;

                Self::Map(map)
            }
            Self::MapMatch(map) => {
                let map = futures::stream::iter(map.into_iter().map(
                    |(key, into_key, val, is_id)| async move {
                        let val: OptionFuture<_> =
                            val.map(|expr| async { expr.reduce().await }).into();
                        (key, into_key, val.await, is_id)
                    },
                ))
                .buffer_unordered(4)
                .collect()
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
                futures::stream::iter(lst.into_iter().map(|expr| async move {
                    Self::Contextual(ctx_id, system_id, Box::new(expr))
                        .reduce()
                        .await
                }))
                .buffered(4)
                .collect()
                .await,
            ),
            Self::Map(map) => Self::Map(
                futures::stream::iter(map.into_iter().map(|(key, (expr, is_id))| async move {
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
                .buffer_unordered(4)
                .collect()
                .await,
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

    pub async fn reduce_all(self) -> Self {
        let mut expr = self;
        loop {
            let old_expr = expr.clone();
            expr = expr.reduce().await;

            if old_expr == expr {
                break;
            }
        }

        expr
    }

    #[async_recursion]
    pub async fn replace_args(self, key: &String, value: &Syntax) -> Syntax {
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
                let asgs: Vec<(Syntax, Syntax)> = futures::stream::iter(
                    asgs.into_iter()
                        .map(|(lhs, rhs)| async { (lhs, rhs.replace_args(key, value).await) }),
                )
                .buffered(4)
                .collect()
                .await;

                let expr_false = expr_false.replace_args(key, value);
                if futures::stream::iter(asgs.iter().map(|(_, rhs)| rhs.get_args()))
                    .any(|args| async { args.await.contains(key) })
                    .await
                {
                    Self::IfLet(asgs, expr_true, Box::new(expr_false.await))
                } else {
                    let expr_true = expr_true.replace_args(key, value);
                    Self::IfLet(asgs, Box::new(expr_true.await), Box::new(expr_false.await))
                }
            }
            Self::UnexpectedArguments() => self,
            Self::ValAny() => self,
            Self::ValInt(_) => self,
            Self::ValFlt(_) => self,
            Self::ValStr(_) => self,
            Self::ValAtom(_) => self,
            Self::Lst(lst) => Self::Lst(
                futures::stream::iter(
                    lst.into_iter()
                        .map(|expr| async { expr.replace_args(key, value).await }),
                )
                .buffered(4)
                .collect()
                .await,
            ),
            Self::LstMatch(lst) => Self::LstMatch(
                futures::stream::iter(
                    lst.into_iter()
                        .map(|expr| async { expr.replace_args(key, value).await }),
                )
                .buffered(4)
                .collect()
                .await,
            ),
            Self::Map(map) => {
                let map = futures::stream::iter(map.into_iter().map(
                    |(map_key, (map_val, is_id))| async move {
                        (map_key, (map_val.replace_args(key, value).await, is_id))
                    },
                ))
                .buffer_unordered(4)
                .collect()
                .await;

                Self::Map(map)
            }
            Self::MapMatch(map) => {
                let map = futures::stream::iter(map.into_iter().map(
                    |(map_key, map_into_key, map_val, is_id)| async move {
                        let map_val: OptionFuture<_> = map_val
                            .map(|x| async { x.replace_args(key, value).await })
                            .into();

                        (map_key, map_into_key, map_val.await, is_id)
                    },
                ))
                .buffer_unordered(4)
                .collect()
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
                    join_all(asgs.iter().map(|(lhs, rhs)| async {
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
                    join_all(lst.iter().map(|expr| async { expr.get_args().await }))
                        .await
                        .into_iter()
                        .flatten(),
                );
            }
            Self::LstMatch(lst) => {
                result.extend(
                    join_all(lst.iter().map(|expr| async { expr.get_args().await }))
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
            Self::OpPow => "**",
        })
    }
}

impl std::fmt::Display for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn val_str(x: &str) -> String {
            format!(
                "\"{}\"",
                x.chars()
                    .map(|c| match c {
                        '\\' => "\\".to_string(),
                        '\n' => "\\n".to_string(),
                        '\r' => "\\r".to_string(),
                        '\t' => "\\t".to_string(),
                        '\0' => "\\0".to_string(),
                        '\"' => "\\\"".to_string(),
                        c => format!("{c}"),
                    })
                    .collect::<String>()
            )
        }

        f.write_str(
            match self {
                Self::Program(exprs) => {
                    exprs
                        .iter()
                        .map(|expr| format!("{expr}"))
                        .fold(String::new(), |a, b| {
                            if a.is_empty() {
                                b
                            } else {
                                format!("{a}\n{b}")
                            }
                        })
                }
                Self::Lambda(id, expr) => format!("(\\{id} {expr})"),
                Self::Call(lhs, rhs) => format!("({lhs} {rhs})"),
                Self::Asg(lhs, rhs) => format!("({lhs} = {rhs})"),
                Self::Tuple(lhs, rhs) => format!("({lhs}, {rhs})"),
                Self::Let((lhs, rhs), expr) => format!("(let {lhs} = {rhs} in {expr})"),
                Self::Pipe(expr) => format!("| {expr}"),
                Self::BiOp(BiOpType::OpPeriod, lhs, rhs) => format!("({lhs}.{rhs})"),
                Self::BiOp(op, lhs, rhs) => format!("({lhs} {op} {rhs})"),
                Self::IfLet(asgs, expr_true, expr_false) => format!(
                    "if let {} then {} else {}",
                    asgs.iter().map(|(lhs, rhs)| format!("{lhs} = {rhs}")).fold(
                        String::new(),
                        |x, y| if x.is_empty() { y } else { format!("{x}; {y}") }
                    ),
                    expr_true,
                    expr_false
                ),
                Self::If(cond, lhs, rhs) => format!("if {cond} then {lhs} else {rhs}"),
                Self::Id(id) => id.to_string(),
                Self::UnexpectedArguments() => "UnexpectedArguments".to_string(),
                Self::ValAny() => "_".to_string(),
                Self::ValInt(x) => x.to_string(),
                Self::ValFlt(x) => {
                    let x: BigDecimal = x.clone().into();
                    let result = format!("{x}");
                    if result.contains(".") {
                        result
                            .trim_end_matches(|c| c == '0')
                            .trim_end_matches(|c| c == '.')
                            .to_owned()
                    } else {
                        result
                    }
                }
                .to_string(),
                Self::ValStr(x) => val_str(x),
                Self::ValAtom(x) => format!("@{x}"),
                Self::Lst(lst) => format!(
                    "[{}]",
                    lst.iter()
                        .map(|x| format!("{x}"))
                        .fold(String::new(), |x, y| {
                            if x.is_empty() {
                                y
                            } else {
                                format!("{x}, {y}")
                            }
                        })
                ),
                Self::LstMatch(lst) => format!(
                    "[{}]",
                    lst.iter()
                        .map(|x| format!("{x}"))
                        .fold(String::new(), |x, y| {
                            if x.is_empty() {
                                y
                            } else {
                                format!("{x}:{y}")
                            }
                        })
                ),
                Self::Map(map) => {
                    if map.is_empty() {
                        "{}".to_string()
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
                                        format!("{x}, {y}")
                                    }
                                })
                        )
                    }
                }
                Self::MapMatch(map) => {
                    if map.is_empty() {
                        "{}".to_string()
                    } else {
                        format!(
                            "{{ {} }}",
                            map.iter()
                                .map(|(key, key_into, val, is_id)| {
                                    let key = if let Some(key_into) = key_into {
                                        format!("{} {}", val_str(key), key_into)
                                    } else if *is_id {
                                        key.clone()
                                    } else {
                                        val_str(key)
                                    };

                                    if let Some(val) = val {
                                        format!("{key}: {val}")
                                    } else {
                                        key
                                    }
                                })
                                .fold(String::new(), |x, y| {
                                    if x.is_empty() {
                                        y
                                    } else {
                                        format!("{x}, {y}")
                                    }
                                })
                        )
                    }
                }
                Self::ExplicitExpr(expr) => format!("{expr}"),
                Self::Contextual(_, _, expr) => format!("{expr}"),
                Self::Context(_, _, id) => id.to_string(),
                Self::Signal(_, _) => "signal".to_string(),
            }
            .as_str(),
        )
    }
}