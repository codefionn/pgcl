use bigdecimal::BigDecimal;
use log::debug;
use std::collections::{BTreeMap, HashSet};
use tailcall::tailcall;

use crate::{context::Context, errors::InterpreterError};

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
}

/// Representing a typed syntax tree
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Syntax {
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
    UnexpectedArguments(),
    ValAny(),
    ValInt(/* num: */ num::BigInt),
    ValFlt(/* num: */ BigDecimal),
    ValStr(/* str: */ String),
    ValAtom(/* atom: */ String),
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
fn int_to_flt(x: num::BigInt) -> BigDecimal {
    x.to_string().parse().unwrap()
}

impl Syntax {
    /// This method reduces the function with very little outside information.
    ///
    /// Should be used for optimizing an expressing before evaluating it.
    pub fn reduce(self) -> Self {
        match self {
            Self::Id(_) => self,
            Self::ValAny() => self,
            Self::ValInt(_) => self,
            Self::ValFlt(_) => self,
            Self::ValStr(_) => self,
            Self::ValAtom(_) => self,
            Self::Pipe(_) => self,
            Self::Lambda(id, expr) => Self::Lambda(id, Box::new(expr.reduce())),
            Self::IfLet(asgs, expr_true, expr_false) => {
                let mut expr_true = expr_true;
                let mut new_asgs = Vec::new();
                for asg in asgs.into_iter() {
                    if let (Self::Id(lhs), Self::Id(rhs)) = asg {
                        expr_true = Box::new(expr_true.replace_args(&lhs, &Self::Id(rhs)));
                    } else {
                        new_asgs.push(asg);
                    }
                }

                if new_asgs.is_empty() {
                    expr_true.reduce()
                } else {
                    Self::IfLet(
                        new_asgs,
                        Box::new(expr_true.reduce()),
                        Box::new(expr_false.reduce()),
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
            Self::Tuple(a, b) => Self::Tuple(Box::new(a.reduce()), Box::new(b.reduce())),
            Self::Call(lhs, rhs) => Self::Call(Box::new(lhs.reduce()), Box::new(rhs.reduce())),
            Self::Let((box Self::Id(id), rhs), expr) => expr.replace_args(&id, &rhs),
            Self::Let(_, _) => self,
            Self::Asg(lhs, rhs) => Self::Asg(lhs, Box::new(rhs.reduce())),
            Self::If(cond, lhs, rhs) => Self::If(
                Box::new(cond.reduce()),
                Box::new(lhs.reduce()),
                Box::new(rhs.reduce()),
            ),
            Self::BiOp(op, a, b) => Self::BiOp(op, Box::new(a.reduce()), Box::new(b.reduce())),
            Self::Lst(lst) => Self::Lst(lst.into_iter().map(|expr| expr.reduce()).collect()),
            Self::LstMatch(lst) => {
                Self::LstMatch(lst.into_iter().map(|expr| expr.reduce()).collect())
            }
            Self::Map(map) => {
                let map = map
                    .into_iter()
                    .map(|(key, (val, is_id))| (key, (val.reduce(), is_id)))
                    .collect();
                Self::Map(map)
            }
            Self::MapMatch(map) => {
                let map = map
                    .into_iter()
                    .map(|(key, into_key, val, is_id)| {
                        (key, into_key, val.map(|expr| expr.reduce()), is_id)
                    })
                    .collect();
                Self::MapMatch(map)
            }
            Self::ExplicitExpr(expr) => expr.reduce(),
            Self::UnexpectedArguments() => self,
        }
    }

    fn replace_args(self, key: &String, value: &Syntax) -> Syntax {
        match self {
            Self::Id(id) if *id == *key => value.clone(),
            Self::Id(_) => self,
            Self::Lambda(id, expr) if *id != *key => {
                Self::Lambda(id, Box::new(expr.replace_args(key, value)))
            }
            Self::Lambda(_, _) => self,
            Self::Call(lhs, rhs) => {
                let lhs = lhs.replace_args(key, value);
                let rhs = rhs.replace_args(key, value);
                Self::Call(Box::new(lhs), Box::new(rhs))
            }
            Self::Asg(lhs, rhs) => {
                let lhs = lhs.replace_args(key, value);
                let rhs = rhs.replace_args(key, value);
                Self::Asg(Box::new(lhs), Box::new(rhs))
            }
            Self::Tuple(lhs, rhs) => {
                let lhs = lhs.replace_args(key, value);
                let rhs = rhs.replace_args(key, value);
                Self::Tuple(Box::new(lhs), Box::new(rhs))
            }
            Self::Let((lhs, rhs), expr) => {
                let rhs = rhs.replace_args(key, value);
                if lhs.get_args().contains(key) {
                    Self::Let((lhs, Box::new(rhs)), expr)
                } else {
                    let expr = expr.replace_args(key, value);
                    Self::Let((lhs, Box::new(rhs)), Box::new(expr))
                }
            }
            Self::BiOp(op, lhs, rhs) => {
                let lhs = lhs.replace_args(key, value);
                let rhs = rhs.replace_args(key, value);
                Self::BiOp(op, Box::new(lhs), Box::new(rhs))
            }
            Self::If(cond, expr_true, expr_false) => {
                let cond = cond.replace_args(key, value);
                let expr_true = expr_true.replace_args(key, value);
                let expr_false = expr_false.replace_args(key, value);
                Self::If(Box::new(cond), Box::new(expr_true), Box::new(expr_false))
            }
            Self::IfLet(asgs, expr_true, expr_false) => {
                let asgs: Vec<(Syntax, Syntax)> = asgs
                    .into_iter()
                    .map(|(lhs, rhs)| (lhs, rhs.replace_args(key, value)))
                    .collect();

                let expr_false = expr_false.replace_args(key, value);
                if asgs
                    .iter()
                    .map(|(_, rhs)| rhs.get_args())
                    .flatten()
                    .collect::<HashSet<String>>()
                    .contains(key)
                {
                    Self::IfLet(asgs, expr_true, Box::new(expr_false))
                } else {
                    let expr_true = expr_true.replace_args(key, value);
                    Self::IfLet(asgs, Box::new(expr_true), Box::new(expr_false))
                }
            }
            Self::UnexpectedArguments() => self,
            Self::ValAny() => self,
            Self::ValInt(_) => self,
            Self::ValFlt(_) => self,
            Self::ValStr(_) => self,
            Self::ValAtom(_) => self,
            Self::Lst(lst) => Self::Lst(
                lst.into_iter()
                    .map(|expr| expr.replace_args(key, value))
                    .collect(),
            ),
            Self::LstMatch(lst) => Self::LstMatch(
                lst.into_iter()
                    .map(|expr| expr.replace_args(key, value))
                    .collect(),
            ),
            Self::Map(map) => {
                let map = map
                    .into_iter()
                    .map(|(map_key, (map_val, is_id))| {
                        (map_key, (map_val.replace_args(&key, value), is_id))
                    })
                    .collect();
                Self::Map(map)
            }
            Self::MapMatch(map) => {
                let map = map
                    .into_iter()
                    .map(|(map_key, map_into_key, map_val, is_id)| {
                        (
                            map_key,
                            map_into_key,
                            map_val.map(|x| x.replace_args(&key, value)),
                            is_id,
                        )
                    })
                    .collect();
                Self::MapMatch(map)
            }
            Self::ExplicitExpr(expr) => Self::ExplicitExpr(Box::new(expr.replace_args(key, value))),
            Self::Pipe(expr) => Self::Pipe(Box::new(expr.replace_args(key, value))),
        }
    }

    fn get_args(&self) -> HashSet<String> {
        let mut result = HashSet::new();
        match self {
            Self::Id(id) => {
                result.insert(id.clone());
            }
            Self::Lambda(_, expr) => {
                result.extend(expr.get_args());
            }
            Self::Call(lhs, rhs)
            | Self::Asg(lhs, rhs)
            | Self::Tuple(lhs, rhs)
            | Self::BiOp(_, lhs, rhs) => {
                result.extend(lhs.get_args());
                result.extend(rhs.get_args());
            }
            Self::Let((lhs, rhs), expr) => {
                result.extend(lhs.get_args());
                result.extend(rhs.get_args());
                result.extend(expr.get_args());
            }
            Self::If(cond, expr_true, expr_false) => {
                result.extend(cond.get_args());
                result.extend(expr_true.get_args());
                result.extend(expr_false.get_args());
            }
            Self::IfLet(asgs, expr_true, expr_false) => {
                result.extend(
                    asgs.iter()
                        .map(|(lhs, rhs)| {
                            let mut result = lhs.get_args();
                            result.extend(rhs.get_args());

                            result
                        })
                        .flatten(),
                );

                result.extend(expr_true.get_args());
                result.extend(expr_false.get_args());
            }
            Self::UnexpectedArguments()
            | Self::ValAny()
            | Self::ValInt(_)
            | Self::ValFlt(_)
            | Self::ValStr(_)
            | Self::ValAtom(_) => {}
            Self::Lst(lst) => {
                result.extend(lst.into_iter().map(|expr| expr.get_args()).flatten());
            }
            Self::LstMatch(lst) => {
                result.extend(lst.into_iter().map(|expr| expr.get_args()).flatten());
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
                result.extend(expr.get_args());
            }
            Self::Pipe(expr) => result.extend(expr.get_args()),
        }

        result
    }

    fn execute_once(self, first: bool, ctx: &mut Context) -> Result<Syntax, InterpreterError> {
        let mut values_defined_here = Vec::new();
        match self.clone().reduce() {
            Self::Id(id) => Ok(if let Some(obj) = ctx.get_from_values(&id, &mut values_defined_here) {
                obj.clone()
            } else {
                self
            }),
            Self::ValAny() => Ok(self),
            Self::ValInt(_) => Ok(self),
            Self::ValFlt(_) => Ok(self),
            Self::ValStr(_) => Ok(self),
            Self::ValAtom(_) => Ok(self),
            Self::Lambda(_, _) => Ok(self),
            Self::Pipe(_) => Ok(self),
            Self::IfLet(asgs, expr_true, expr_false) => {
                for (lhs, rhs) in asgs {
                    let rhs = rhs.clone().execute(false, ctx)?;
                    if !ctx.set_values_in_context(
                        &lhs,
                        &rhs,
                        &mut values_defined_here,
                    ) {
                        ctx.remove_values(&mut values_defined_here);
                        return expr_false.execute_once(false, ctx)
                    }
                }

                let mut result = expr_true.clone();
                for (key, value) in ctx.remove_values(&mut values_defined_here).into_iter() {
                    result = Box::new(result.replace_args(&key, &value));
                }

                Ok(*result)
            }
            Self::Call(box Syntax::Id(id), body) => {
                if let Some(value) = ctx.get_from_values(&id, &mut values_defined_here) {
                    Ok(Self::Call(Box::new(value), body))
                } else {
                    Ok(self)
                }
            }
            Self::Call(box Syntax::Lambda(id, fn_expr), expr) => {
                /*insert_into_values(&id, expr.execute(false, ctx)?, ctx);

                let result = fn_expr.execute(false, ctx);
                remove_values(context, &values_defined_here);

                result*/
                Ok(fn_expr.replace_args(&id, &expr))
            }
            Self::Call(lhs, rhs) => {
                let new_lhs = lhs.clone().execute(false, ctx)?;
                if new_lhs == *lhs {
                    Ok(Self::Call(lhs, Box::new(rhs.execute_once(false, ctx)?)))
                } else {
                    Self::Call(Box::new(new_lhs), rhs).execute_once(false, ctx)
                }
            }
            Self::Asg(box Self::Id(id), rhs) => {
                if !first {
                    Ok(self)
                } else {
                    ctx.insert_into_values(&id, *rhs, &mut values_defined_here);
                    Ok(Self::ValAny())
                }
            }
            Self::Asg(lhs, rhs) => {
                if !first {
                    Ok(self)
                } else {
                    #[tailcall]
                    fn extract_id(
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

                    fn unpack_params(
                        syntax: Syntax,
                        mut result: Vec<Syntax>,
                    ) -> Result<Vec<Syntax>, InterpreterError> {
                        match syntax {
                            Syntax::Call(a, b) => {
                                result.insert(0, *b);
                                unpack_params(*a, result)
                            }
                            _ => {
                                result.insert(0, syntax);
                                Ok(result)
                            }
                        }
                    }

                    let (id, syntax) = extract_id(*lhs, Vec::new())?;
                    ctx.insert_fns(
                        id,
                        (unpack_params(syntax, Default::default())?, rhs.reduce()),
                    );

                    Ok(Syntax::ValAny())
                }
            }
            Self::Let((lhs, rhs), expr) => {
                if !ctx.set_values_in_context(&lhs, &rhs, &mut values_defined_here) {
                    Ok(*expr)
                } else {
                    let mut result = (*expr).clone();
                    for (key, value) in ctx.remove_values(&mut values_defined_here).into_iter() {
                        result = result.replace_args(&key, &value);
                    }

                    Ok(result)
                }
            }
            Self::BiOp(BiOpType::OpGeq, box Self::ValInt(x), box Self::ValInt(y)) => {
                Ok((x >= y).into())
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValInt(x), box Self::ValInt(y)) => {
                Ok((x <= y).into())
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValInt(x), box Self::ValInt(y)) => {
                Ok((x > y).into())
            }
            Self::BiOp(BiOpType::OpLt, box Self::ValInt(x), box Self::ValInt(y)) => {
                Ok((x < y).into())
            }
            Self::BiOp(BiOpType::OpGeq, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Ok((x >= y).into())
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Ok((x <= y).into())
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Ok((x > y).into())
            }
            Self::BiOp(BiOpType::OpLt, box Self::ValFlt(x), box Self::ValFlt(y)) => {
                Ok((x < y).into())
            }
            Self::BiOp(BiOpType::OpGeq, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Ok((int_to_flt(x) >= y).into())
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Ok((int_to_flt(x) <= y).into())
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Ok((int_to_flt(x) > y).into())
            }
            Self::BiOp(BiOpType::OpLt, box Self::ValInt(x), box Self::ValFlt(y)) => {
                Ok((int_to_flt(x) < y).into())
            }
            Self::BiOp(BiOpType::OpGeq, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Ok((x >= int_to_flt(y)).into())
            }
            Self::BiOp(BiOpType::OpLeq, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Ok((x <= int_to_flt(y)).into())
            }
            Self::BiOp(BiOpType::OpGt, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Ok((x > int_to_flt(y)).into())
            }
            Self::BiOp(BiOpType::OpLt, box Self::ValFlt(x), box Self::ValInt(y)) => {
                Ok((x < int_to_flt(y)).into())
            }
            Self::BiOp(op @ BiOpType::OpGeq, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpLeq, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpGt, lhs, rhs)
            | Self::BiOp(op @ BiOpType::OpLt, lhs, rhs) => {
                let lhs = lhs.execute_once(false, ctx)?;
                let rhs = rhs.execute_once(false, ctx)?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Self::BiOp(BiOpType::OpEq, lhs, rhs) => {
                let lhs = lhs.execute(false, ctx)?;
                let rhs = rhs.execute(false, ctx)?;

                Ok(if lhs.eval_equal(&rhs) {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(BiOpType::OpNeq, lhs, rhs) => {
                let lhs = lhs.execute(false, ctx)?;
                let rhs = rhs.execute(false, ctx)?;

                Ok(if !lhs.eval_equal(&rhs) {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(op, lhs, rhs) => {
                let lhs = lhs.execute_once(false, ctx)?;
                let rhs = rhs.execute_once(false, ctx)?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Self::If(cond, expr_true, expr_false) => {
                let cond = cond.execute(false, ctx)?;
                Ok(match cond {
                    Self::ValAtom(id) if id == "true" => expr_true.execute_once(false, ctx)?,
                    Self::ValAtom(id) if id == "false" => {
                        expr_false.execute_once(false, ctx)?
                    }
                    _ => {
                        ctx
                            .errors
                            .push(format!("Expected :true or :false in if-condition"));
                        self
                    }
                })
            }
            Self::Tuple(lhs, rhs) => {
                let lhs = lhs.execute_once(false, ctx)?;
                let rhs = rhs.execute_once(false, ctx)?;

                Ok(Self::Tuple(Box::new(lhs), Box::new(rhs)))
            }
            Self::UnexpectedArguments() => {
                ctx.errors.push(format!("Unexpected arguments"));
                Ok(self)
            }
            Self::Lst(lst) => {
                let mut result = Vec::new();
                for e in lst {
                    result.push(e.execute_once(false, ctx)?);
                }

                Ok(Self::Lst(result))
            }
            Self::LstMatch(lst) => {
                let mut result = Vec::new();
                for e in lst {
                    result.push(e.execute_once(false, ctx)?);
                }

                Ok(Self::LstMatch(result))
            }
            Self::Map(map) => {
                let map = map
                    .into_iter()
                    .map(
                        |(key, (val, is_id))| -> Result<(String, (Syntax, bool)), InterpreterError> {
                            Ok((key, (val.execute_once(false, ctx)?, is_id)))
                        },
                    )
                    .try_collect()?;

                Ok(Self::Map(map))
            }
            Self::MapMatch(_) => Ok(self),
            Self::ExplicitExpr(box expr) => Ok(expr),
        }
        .map(|expr| expr.reduce())
    }

    pub fn execute(self, first: bool, ctx: &mut Context) -> Result<Syntax, InterpreterError> {
        let mut this = self;
        let mut old = this.clone();
        loop {
            this = this.execute_once(first, ctx)?;

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

    pub fn eval_equal(&self, other: &Self) -> bool {
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
            (Syntax::ValFlt(a), Syntax::ValInt(b)) => *a == b.to_string().parse().unwrap(),
            (Syntax::ValInt(b), Syntax::ValFlt(a)) => *a == b.to_string().parse().unwrap(),
            (Syntax::ValStr(a), Syntax::ValStr(b)) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Display for BiOpType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
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
                Self::Lambda(id, expr) => format!("(\\{} {})", id, expr.to_string()),
                Self::Call(lhs, rhs) => format!("({} {})", lhs, rhs),
                Self::Asg(lhs, rhs) => format!("({} = {})", lhs.to_string(), rhs.to_string()),
                Self::Tuple(lhs, rhs) => format!("({}, {})", lhs.to_string(), rhs.to_string()),
                Self::Let((lhs, rhs), expr) => format!("(let {} = {} in {})", lhs, rhs, expr),
                Self::Pipe(expr) => format!("| {}", expr),
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
                Self::ValFlt(x) => format!("{}", x)
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
                Self::Map(map) => format!(
                    "{{{}}}",
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
                ),
                Self::MapMatch(map) => format!(
                    "{{{}}}",
                    map.iter()
                        .map(|(key, key_into, val, is_id)| {
                            let key = if let Some(key_into) = key_into {
                                format!("{} {}", val_str(key), key_into)
                            } else {
                                format!("{}", if *is_id { key.clone() } else { val_str(key) })
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
                ),
                Self::ExplicitExpr(expr) => format!("{}", expr),
            }
            .as_str(),
        )
    }
}
