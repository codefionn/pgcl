use bevy::{prelude::*, utils::HashSet};

use bigdecimal::BigDecimal;
use std::collections::HashMap;
use tailcall::tailcall;

use crate::errors::InterpreterError;

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
}

/// Representing a typed syntax tree
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Syntax {
    Lambda(/* id: */ String, /* expr: */ Box<Syntax>),
    Call(/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
    Asg(/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
    Tuple(/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
    Let(
        /* asg: */ (/* rhs: */ Box<Syntax>, /* lhs: */ Box<Syntax>),
        /* expr: */ Box<Syntax>,
    ),
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
    UnexpectedArguments(),
    ValAny(),
    ValInt(/* num: */ num::BigInt),
    ValFlt(/* num: */ BigDecimal),
    ValStr(/* str: */ String),
    ValAtom(/* atom: */ String),
}

#[derive(Default)]
pub struct Context {
    /// HashMap with a stack of values
    values: HashMap<String, Vec<Syntax>>,
    fns: HashMap<String, Vec<(Vec<Syntax>, Syntax)>>,
    errors: Vec<String>,
}

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
            Self::Asg(lhs, rhs) => Self::Asg(Box::new(lhs.reduce()), Box::new(rhs.reduce())),
            Self::If(cond, lhs, rhs) => Self::If(
                Box::new(cond.reduce()),
                Box::new(lhs.reduce()),
                Box::new(rhs.reduce()),
            ),
            Self::BiOp(op, a, b) => Self::BiOp(op, Box::new(a.reduce()), Box::new(b.reduce())),
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
        }

        result
    }

    fn execute_once(self, first: bool, context: &mut Context) -> Result<Syntax, InterpreterError> {
        let mut values_defined_here = Vec::new();
        match self.clone().reduce() {
            Self::Id(id) => Ok(if let Some(obj) = get_from_values(&id, context) {
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
            Self::IfLet(asgs, expr_true, expr_false) => {
                let mut cond_result = true;
                for (lhs, rhs) in asgs {
                    if !set_values_in_context(
                        &lhs,
                        &rhs.clone().execute(false, context)?,
                        &mut values_defined_here,
                        context,
                    ) {
                        cond_result = false;
                        remove_values(context, &values_defined_here);
                        break;
                    }
                }

                if cond_result {
                    let mut result = expr_true.clone();
                    for (key, value) in remove_values(context, &values_defined_here).into_iter() {
                        result = Box::new(result.replace_args(&key, &value));
                    }

                    Ok(*result)
                } else {
                    expr_false.execute(false, context)
                }
            }
            Self::Call(box Syntax::Id(id), body) => {
                if let Some(value) = get_from_values(&id, context) {
                    Self::Call(Box::new(value), body).execute(false, context)
                } else {
                    Ok(self)
                }
            }
            Self::Call(box Syntax::Lambda(id, fn_expr), expr) => {
                /*insert_into_values(&id, expr.execute(false, context)?, context);

                let result = fn_expr.execute(false, context);
                remove_values(context, &values_defined_here);

                result*/
                Ok(fn_expr.replace_args(&id, &expr))
            }
            Self::Call(lhs, rhs) => {
                let new_lhs = lhs.clone().execute(false, context)?;
                if new_lhs == *lhs {
                    Ok(self)
                } else {
                    Self::Call(Box::new(new_lhs), rhs).execute(false, context)
                }
            }
            Self::Asg(box Self::Id(id), rhs) => {
                if !first {
                    Ok(self)
                } else {
                    insert_into_values(&id, *rhs, context);
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
                    insert_fns(
                        id,
                        (unpack_params(syntax, Default::default())?, rhs.reduce()),
                        context,
                    );

                    Ok(Syntax::ValAny())
                }
            }
            Self::Let((lhs, rhs), expr) => {
                set_values_in_context(&lhs, &rhs, &mut values_defined_here, context);

                let mut result = (*expr).clone();
                for (key, value) in remove_values(context, &values_defined_here).into_iter() {
                    result = result.replace_args(&key, &value);
                }

                Ok(result)
            }
            Self::BiOp(BiOpType::OpEq, lhs, rhs) => {
                let lhs = lhs.execute(false, context)?;
                let rhs = rhs.execute(false, context)?;

                Ok(if lhs.eval_equal(&rhs) {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(BiOpType::OpNeq, lhs, rhs) => {
                let lhs = lhs.execute(false, context)?;
                let rhs = rhs.execute(false, context)?;

                Ok(if !lhs.eval_equal(&rhs) {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(op, lhs, rhs) => {
                let lhs = lhs.execute_once(false, context)?;
                let rhs = rhs.execute_once(false, context)?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Self::If(cond, expr_true, expr_false) => {
                let cond = cond.execute(false, context)?;
                Ok(match cond {
                    Self::ValAtom(id) if id == "true" => expr_true.execute_once(false, context)?,
                    Self::ValAtom(id) if id == "false" => {
                        expr_false.execute_once(false, context)?
                    }
                    _ => {
                        context
                            .errors
                            .push(format!("Expected :true or :false in if-condition"));
                        self
                    }
                })
            }
            Self::Tuple(lhs, rhs) => {
                let lhs = lhs.execute_once(false, context)?;
                let rhs = rhs.execute_once(false, context)?;

                Ok(Self::Tuple(Box::new(lhs), Box::new(rhs)))
            }
            Self::UnexpectedArguments() => {
                context.errors.push(format!("Unexpected arguments"));
                Ok(self)
            }
        }
        .map(|expr| expr.reduce())
    }

    pub fn execute(self, first: bool, context: &mut Context) -> Result<Syntax, InterpreterError> {
        let mut this = self;
        let mut old = this.clone();
        loop {
            this = this.execute_once(first, context)?;

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

    fn eval_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Syntax::Tuple(a0, b0), Syntax::Tuple(a1, b1)) => a0 == a1 && b0 == b1,
            (Syntax::Call(a0, b0), Syntax::Call(a1, b1)) => a0 == a1 && b0 == b1,
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
        })
    }
}

impl std::fmt::Display for Syntax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            match self {
                Self::Lambda(id, expr) => format!("(\\{} {})", id, expr.to_string()),
                Self::Call(lhs, rhs) => format!("{} {}", lhs, rhs),
                Self::Asg(lhs, rhs) => format!("({} = {})", lhs.to_string(), rhs.to_string()),
                Self::Tuple(lhs, rhs) => format!("({}, {})", lhs.to_string(), rhs.to_string()),
                Self::Let((lhs, rhs), expr) => format!("(let {} = {} in {})", lhs, rhs, expr),
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
                Self::ValStr(x) => format!("{}", x),
                Self::ValAtom(x) => format!(":{}", x),
            }
            .as_str(),
        )
    }
}

fn insert_into_values(id: &String, syntax: Syntax, ctx: &mut Context) {
    if let Some(stack) = ctx.values.get_mut(id) {
        stack.push(syntax);
    } else {
        ctx.values.insert(id.clone(), Default::default());
        let stack = ctx.values.get_mut(id).unwrap();
        stack.push(syntax);
    }
}

fn insert_fns(id: String, syntax: (Vec<Syntax>, Syntax), ctx: &mut Context) {
    if let Some(stack) = ctx.fns.get_mut(&id) {
        stack.push(syntax);
    } else {
        ctx.fns.insert(id.clone(), Default::default());
        let stack = ctx.fns.get_mut(&id).unwrap();
        stack.push(syntax);
    }
}

fn remove_values(ctx: &mut Context, values_defined_here: &Vec<String>) -> HashMap<String, Syntax> {
    let mut result = HashMap::with_capacity(values_defined_here.len());

    for id in values_defined_here {
        if let Some(stack) = ctx.values.get_mut(id) {
            if let Some(value) = stack.pop() {
                if result.contains_key(id) {
                    result.insert(id.clone(), value);
                }
            }
        }
    }

    result
}

fn build_fn(name: &String, fns: &[(Vec<Syntax>, Syntax)]) -> Syntax {
    let arg_name = {
        let name = name.clone();
        move |arg: usize| format!("{}{}_{}", name.len(), name, arg)
    };

    fn build_fn_part(params: &[String], fns: &[(Vec<Syntax>, Syntax)]) -> Syntax {
        if fns.is_empty() {
            Syntax::UnexpectedArguments()
        } else {
            let (args, body) = &fns[fns.len() - 1];
            let mut asgs = Vec::new();
            for i in 0..args.len() {
                asgs.push((args[i].clone(), Syntax::Id(params[i].clone())));
            }

            Syntax::IfLet(
                asgs,
                Box::new(body.clone()),
                Box::new(build_fn_part(params, &fns[..fns.len() - 1])),
            )
        }
    }

    fn build_fn_with_args(
        params: &[String],
        all_params: &[String],
        fns: &[(Vec<Syntax>, Syntax)],
    ) -> Syntax {
        if params.is_empty() {
            build_fn_part(all_params, fns)
        } else {
            Syntax::Lambda(
                params[params.len() - 1].clone(),
                Box::new(build_fn_with_args(
                    &params[..params.len() - 1],
                    all_params,
                    fns,
                )),
            )
        }
    }

    let params: Vec<String> = (0..fns[0].0.len()).map(|i| arg_name(i)).collect();

    let mut fns: Vec<(Vec<Syntax>, Syntax)> = fns.iter().map(|x| x.clone()).collect();
    fns.reverse();

    build_fn_with_args(&params, &params, &fns).reduce()
}

fn get_from_values(id: &String, ctx: &mut Context) -> Option<Syntax> {
    if let Some(stack) = ctx.values.get(id) {
        stack.last().map(|syntax| syntax.clone())
    } else {
        if let Some(fns) = ctx.fns.get(id) {
            let compiled_fn = build_fn(id, fns);
            insert_into_values(id, compiled_fn.clone(), ctx);
            Some(compiled_fn)
        } else {
            None
        }
    }
}

fn set_values_in_context(
    lhs: &Syntax,
    rhs: &Syntax,
    values_defined_here: &mut Vec<String>,
    ctx: &mut Context,
) -> bool {
    match (lhs, rhs) {
        (Syntax::Tuple(a0, b0), Syntax::Tuple(a1, b1)) => {
            let a = set_values_in_context(a0, a1, values_defined_here, ctx);
            let b = set_values_in_context(b0, b1, values_defined_here, ctx);
            a && b
        }
        (Syntax::Call(a0, b0), Syntax::Call(a1, b1)) => {
            let a = set_values_in_context(a0, a1, values_defined_here, ctx);
            let b = set_values_in_context(b0, b1, values_defined_here, ctx);
            a && b
        }
        (Syntax::ValAny(), _) => true,
        (Syntax::Id(id), expr) => {
            insert_into_values(id, expr.clone(), ctx);
            true
        }
        (Syntax::BiOp(op0, a0, b0), Syntax::BiOp(op1, a1, b1)) => {
            if op0 != op1 {
                false
            } else {
                let a = set_values_in_context(a0, a1, values_defined_here, ctx);
                let b = set_values_in_context(b0, b1, values_defined_here, ctx);
                a && b
            }
        }
        (expr0, expr1) => expr0.eval_equal(expr1),
    }
}
