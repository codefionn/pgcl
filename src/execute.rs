use std::collections::HashMap;

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
#[derive(Clone, Debug, Eq, Hash)]
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
    ValAny(),
    ValInt(/* num: */ num::BigInt),
    ValFlt(/* num: */ num::BigRational),
    ValStr(/* str: */ String),
    ValAtom(/* atom: */ String),
}

#[derive(Default)]
pub struct Context {
    /// HashMap with a stack of values
    values: HashMap<String, Vec<Syntax>>,
    fns: HashMap<String, Vec<(Syntax, Syntax)>>,
    errors: Vec<String>,
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
            Self::Lambda(_, _) => self,
            Self::IfLet(_, _, _) => self,
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
                Self::ValFlt(num::BigRational::new(x, y))
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
            Self::Call(box Self::Lambda(id, box Self::Id(id_in_expr)), box expr)
                if id == id_in_expr =>
            {
                expr
            }
            Self::Tuple(box a, box b) => Self::Tuple(Box::new(a.reduce()), Box::new(b.reduce())),
            Self::Call(_, _) => self,
            Self::Let(_, _) => self,
            Self::Asg(_, _) => self,
            Self::If(_, _, _) => self,
            Self::BiOp(op, box a, box b) => {
                Self::BiOp(op, Box::new(a.reduce()), Box::new(b.reduce()))
            }
        }
    }

    pub fn execute(self, first: bool, context: &mut Context) -> Result<Syntax, anyhow::Error> {
        let mut values_defined_here = Vec::new();
        let this = self.reduce();
        match this.clone() {
            Self::Id(id) => Ok(if let Some(obj) = get_from_values(&id, context) {
                obj.clone()
            } else {
                this
            }),
            Self::ValAny() => Ok(this),
            Self::ValInt(_) => Ok(this),
            Self::ValFlt(_) => Ok(this),
            Self::ValStr(_) => Ok(this),
            Self::ValAtom(_) => Ok(this),
            Self::Lambda(_, _) => Ok(this),
            Self::IfLet(asgs, box expr_true, box expr_false) => {
                let mut cond_result = true;
                for (lhs, rhs) in asgs {
                    if !set_values_in_context(&lhs, &rhs, &mut values_defined_here, context) {
                        cond_result = false;
                        remove_values(context, &values_defined_here);
                    }
                }

                Ok(if cond_result {
                    let result = expr_true.execute(false, context)?;
                    remove_values(context, &values_defined_here);
                    result
                } else {
                    expr_false.execute(false, context)?
                })
            }
            Self::Call(box Syntax::Lambda(id, fn_expr), box expr) => {
                insert_into_values(&id, expr, context, &mut values_defined_here);

                let result = fn_expr.execute(false, context);
                remove_values(context, &values_defined_here);

                result
            }
            Self::Call(_, _) => Ok(this),
            Self::Asg(box Self::Id(id), box rhs) => {
                if !first {
                    Ok(this)
                } else {
                    insert_into_values(&id, rhs, context, &mut values_defined_here);
                    Ok(Self::ValAny())
                }
            }
            Self::Asg(box lhs, box rhs) => {
                if !first {
                    Ok(this)
                } else {
                    fn extract_id(
                        syntax: Syntax,
                        mut unpacked: Vec<Syntax>,
                    ) -> Result<(String, Syntax), anyhow::Error> {
                        match syntax {
                            Syntax::Call(box Syntax::Id(id), box rhs) => {
                                unpacked.push(rhs);

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
                            Syntax::Call(box lhs, box rhs) => {
                                unpacked.push(rhs.clone());
                                extract_id(lhs.clone(), unpacked)
                            }
                            _ => Err(anyhow::anyhow!("Expected call")),
                        }
                    }

                    let (id, syntax) = extract_id(lhs, Vec::new())?;
                    insert_fns(id, (syntax, rhs), context, &mut values_defined_here);

                    Ok(Syntax::ValAny())
                }
            }
            Self::Let((box lhs, box rhs), box expr) => {
                set_values_in_context(&lhs, &rhs, &mut values_defined_here, context);

                let result = expr.execute(false, context);
                remove_values(context, &values_defined_here);

                result
            }
            Self::BiOp(BiOpType::OpEq, box lhs, box rhs) => {
                let lhs = lhs.execute(false, context)?;
                let rhs = rhs.execute(false, context)?;

                Ok(if lhs == rhs {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(BiOpType::OpNeq, box lhs, box rhs) => {
                let lhs = lhs.execute(false, context)?;
                let rhs = rhs.execute(false, context)?;

                Ok(if lhs != rhs {
                    Self::ValAtom("true".to_string())
                } else {
                    Self::ValAtom("false".to_string())
                })
            }
            Self::BiOp(op, box lhs, box rhs) => {
                let lhs = lhs.execute(false, context)?;
                let rhs = rhs.execute(false, context)?;

                Ok(Self::BiOp(op, Box::new(lhs), Box::new(rhs)).reduce())
            }
            Self::If(box cond, box expr_true, box expr_false) => {
                let cond = cond.execute(false, context)?;
                Ok(match cond {
                    Self::ValAtom(id) if id == "true" => expr_true.execute(false, context)?,
                    Self::ValAtom(id) if id == "false" => expr_false.execute(false, context)?,
                    _ => {
                        context
                            .errors
                            .push(format!("Expected :true or :false in if-condition"));
                        this
                    }
                })
            }
            Self::Tuple(box lhs, box rhs) => {
                let lhs = lhs.execute(false, context)?;
                let rhs = rhs.execute(false, context)?;

                Ok(Self::Tuple(Box::new(lhs), Box::new(rhs)))
            }
        }
    }
}

impl PartialEq for Syntax {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Syntax::Tuple(a0, b0), Syntax::Tuple(a1, b1)) => a0 == a1 && b0 == b1,
            (Syntax::Call(a0, b0), Syntax::Call(a1, b1)) => a0 == a1 && b0 == b1,
            (Syntax::ValAny(), _) => true,
            (_, Syntax::ValAny()) => true,
            (Syntax::ValAtom(a), Syntax::ValAtom(b)) => a == b,
            (Syntax::ValInt(a), Syntax::ValInt(b)) => a == b,
            (Syntax::ValFlt(a), Syntax::ValFlt(b)) => a == b,
            (Syntax::ValStr(a), Syntax::ValStr(b)) => a == b,
            _ => false,
        }
    }
}

fn insert_into_values(
    id: &String,
    syntax: Syntax,
    ctx: &mut Context,
    values_defined_here: &mut Vec<String>,
) {
    if let Some(stack) = ctx.values.get_mut(id) {
        stack.push(syntax);
    } else {
        ctx.values.insert(id.clone(), Default::default());
        let stack = ctx.values.get_mut(id).unwrap();
        stack.push(syntax);
    }
}

fn insert_fns(
    id: String,
    syntax: (Syntax, Syntax),
    ctx: &mut Context,
    values_defined_here: &mut Vec<String>,
) {
    if let Some(stack) = ctx.fns.get_mut(&id) {
        stack.push(syntax);
    } else {
        ctx.fns.insert(id.clone(), Default::default());
        let stack = ctx.fns.get_mut(&id).unwrap();
        stack.push(syntax);
    }
}

fn remove_values(ctx: &mut Context, values_defined_here: &Vec<String>) {
    for id in values_defined_here {
        if let Some(stack) = ctx.values.get_mut(id) {
            stack.pop();
        }
    }
}

fn get_from_values(id: &String, ctx: &mut Context) -> Option<Syntax> {
    if let Some(stack) = ctx.values.get(id) {
        stack.last().map(|syntax| syntax.clone())
    } else {
        None
    }
}

fn set_values_in_context(
    lhs: &Syntax,
    rhs: &Syntax,
    values_defined_here: &mut Vec<String>,
    ctx: &mut Context,
) -> bool {
    match (lhs, rhs) {
        (Syntax::Tuple(box a0, box b0), Syntax::Tuple(box a1, box b1)) => {
            let a = set_values_in_context(a0, a1, values_defined_here, ctx);
            let b = set_values_in_context(b0, b1, values_defined_here, ctx);
            a && b
        }
        (Syntax::Call(box a0, box b0), Syntax::Call(box a1, box b1)) => {
            let a = set_values_in_context(a0, a1, values_defined_here, ctx);
            let b = set_values_in_context(b0, b1, values_defined_here, ctx);
            a && b
        }
        (Syntax::ValAny(), _) => true,
        (Syntax::Id(id), expr) => {
            insert_into_values(id, expr.clone(), ctx, values_defined_here);
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
        (expr0, expr1) => expr0 == expr1,
    }
}
