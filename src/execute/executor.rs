use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    actor,
    context::{ContextHandler, ContextHolder, PrivateContext},
    errors::InterpreterError,
    execute::{BiOpType, SignalType, Syntax},
    lexer::Token,
    parser::{Parser, SyntaxKind},
    system::{SystemCallType, SystemHandler},
};
use async_recursion::async_recursion;
use futures::{future::join_all, StreamExt, TryStreamExt};
use log::debug;
use rowan::GreenNodeBuilder;

pub struct Executor<'a, 'b> {
    ctx: &'a mut ContextHandler,
    system: &'b mut SystemHandler,
}

impl<'a, 'b> Executor<'a, 'b> {
    pub fn new(ctx: &'a mut ContextHandler, system: &'b mut SystemHandler) -> Self {
        Self { ctx, system }
    }

    pub fn get_ctx(&mut self) -> &mut ContextHandler {
        self.ctx
    }

    pub fn get_system(&mut self) -> &mut SystemHandler {
        self.system
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
        &mut self,
        expr: Syntax,
        first: bool,
        no_change: bool,
    ) -> Result<Syntax, InterpreterError> {
        // This local context helps by not locking up the current context `ctx`
        let mut local_ctx = PrivateContext::new(usize::MAX, "local".to_string(), None);

        let mut values_defined_here = Vec::new();
        let expr: Syntax = match expr.clone().reduce().await {
            Syntax::Id(id) => Ok(
                if let Some(obj) = self
                    .ctx
                    .get_from_values(&id, &mut values_defined_here)
                    .await
                {
                    if first {
                        if let Ok(obj) = self.execute(obj.clone(), false).await {
                            self.ctx.replace_from_values(&id, obj.clone()).await;

                            obj
                        } else {
                            obj.clone()
                        }
                    } else {
                        obj.clone()
                    }
                } else {
                    match id.as_str() {
                        "false" => false.into(),
                        "true" => true.into(),
                        _ => expr,
                    }
                },
            ),
            Syntax::ValAny() => Ok(expr),
            Syntax::ValInt(_) => Ok(expr),
            Syntax::ValFlt(_) => Ok(expr),
            Syntax::ValStr(_) => Ok(expr),
            Syntax::ValAtom(_) => Ok(expr),
            Syntax::Lambda(_, _) => Ok(expr),
            Syntax::Pipe(_) => Ok(expr),
            Syntax::Program(exprs) => {
                let mut exprs: Vec<_> = futures::stream::iter(exprs.into_iter().map(|expr| {
                    let mut ctx = self.ctx.clone();
                    let mut system = self.system.clone();

                    async move {
                        Executor::new(&mut ctx, &mut system)
                            .execute(expr, first)
                            .await
                    }
                }))
                .buffered(1)
                .try_collect()
                .await?;

                // If the program has at least one expression => return the result of the last one
                if let Some(expr) = exprs.pop() {
                    Ok(expr)
                } else {
                    Ok(Syntax::ValAny())
                }
            }
            Syntax::IfLet(asgs, expr_true, expr_false) => {
                async fn execute_asgs(
                    asgs: Vec<(Syntax, Syntax)>,
                    no_change: bool,
                    ctx: &mut ContextHandler,
                    system: &mut SystemHandler,
                ) -> Result<Vec<(Syntax, Syntax)>, InterpreterError> {
                    futures::stream::iter(asgs.into_iter().map(|(lhs, rhs)| {
                        let mut ctx = ctx.clone();
                        let mut system = system.clone();
                        async move {
                            let mut executor = Executor::new(&mut ctx, &mut system);
                            Ok((
                                executor.execute_once(lhs, false, no_change).await?,
                                executor.execute_once(rhs, false, no_change).await?,
                            ))
                        }
                    }))
                    .buffered(1)
                    .try_collect()
                    .await
                }

                if no_change {
                    let old_asgs = asgs.clone();
                    let asgs = execute_asgs(asgs, true, self.ctx, self.system).await?;

                    if old_asgs == asgs {
                        for (lhs, rhs) in asgs {
                            let rhs = self.execute(rhs.clone(), false).await?;
                            if !local_ctx
                                .set_values_in_context(
                                    &mut self.ctx.get_holder(),
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
                    } else {
                        Ok(Syntax::IfLet(asgs, expr_true, expr_false))
                    }
                } else {
                    let asgs = execute_asgs(asgs, false, self.ctx, self.system).await?;

                    Ok(Syntax::IfLet(asgs, expr_true, expr_false))
                }
            }
            Syntax::Call(box Syntax::Signal(signal_type, signal_id), expr) => match signal_type {
                SignalType::Actor => {
                    if let Some(tx) = self.system.get_holder().get_actor(signal_id).await {
                        if tx
                            .send(actor::Message::Signal(Syntax::Contextual(
                                self.ctx.get_id(),
                                self.system.get_id(),
                                expr,
                            )))
                            .await
                            .is_err()
                        {
                            Ok(Syntax::ValAtom("false".to_string()))
                        } else {
                            Ok(Syntax::ValAtom("true".to_string()))
                        }
                    } else {
                        Ok(Syntax::ValAtom("false".to_string()))
                    }
                }
                SignalType::Message => Ok(
                    match self
                        .system
                        .get_holder()
                        .send_message(
                            signal_id,
                            Syntax::Contextual(self.ctx.get_id(), self.system.get_id(), expr),
                        )
                        .await
                    {
                        Ok(()) => Syntax::ValAtom("success".to_string()),
                        Err(err) => Syntax::Tuple(
                            Box::new(Syntax::ValAtom("error".to_string())),
                            Box::new(Syntax::ValStr(format!("{}", err))),
                        ),
                    },
                ),
            },
            Syntax::Call(box Syntax::Id(id), body) => {
                make_call(
                    self.ctx,
                    self.system,
                    no_change,
                    id,
                    body,
                    expr,
                    &mut values_defined_here,
                )
                .await
            }
            Syntax::Call(box Syntax::Lambda(id, fn_expr), expr) => {
                /*insert_into_values(&id, expr.execute(false, ctx)?, ctx);

                let result = fn_expr.execute(false, ctx);
                remove_values(context, &values_defined_here);

                result*/
                Ok(fn_expr.replace_args(&id, &expr).await)
            }
            Syntax::Call(box Syntax::Contextual(ctx_id, system_id, box Syntax::Id(id)), rhs) => {
                let mut ctx = self.ctx.get_holder().get_handler(ctx_id).await.unwrap();
                let mut system = self
                    .system
                    .get_holder()
                    .get_handler(system_id)
                    .await
                    .unwrap();

                Ok(Syntax::Contextual(
                    ctx_id,
                    system_id,
                    Box::new(
                        make_call(
                            &mut ctx,
                            &mut system,
                            no_change,
                            id,
                            rhs,
                            expr.clone(),
                            &mut values_defined_here,
                        )
                        .await?,
                    ),
                ))
            }
            Syntax::Call(box Syntax::Contextual(ctx_id, system_id, lhs), rhs) => {
                let old_ctx_id = self.ctx.get_id();
                let old_system_id = self.system.get_id();

                let mut ctx = self.ctx.get_holder().get_handler(ctx_id).await.unwrap();
                let mut system = self
                    .system
                    .get_holder()
                    .get_handler(system_id)
                    .await
                    .unwrap();
                let mut executor = Executor::new(&mut ctx, &mut system);

                // Create a new contextual with the contents evaulated in the given context
                // The contextual is reduced away if possible in the next execution step
                Ok(Syntax::Contextual(
                    ctx_id,
                    system_id,
                    Box::new(
                        executor
                            .execute_once(
                                Syntax::Call(
                                    lhs,
                                    Box::new(Syntax::Contextual(old_ctx_id, old_system_id, rhs)),
                                ),
                                false,
                                no_change,
                            )
                            .await?,
                    ),
                ))
            }
            Syntax::Call(lhs, rhs) => {
                let old_lhs = lhs.clone();
                let lhs = self.execute_once(*lhs, false, no_change).await?;
                if lhs == *old_lhs {
                    // Executing the left side doesn't seem to do the trick anymore
                    // => evaulate the right side (this can produces more desirable results)
                    Ok(Syntax::Call(
                        Box::new(lhs),
                        Box::new(self.execute_once(*rhs, false, no_change).await?),
                    ))
                } else {
                    Ok(Syntax::Call(Box::new(lhs), rhs))
                }
            }
            Syntax::Asg(box Syntax::Id(id), rhs) => {
                if !first {
                    // Assignments are only allowed on the top-level
                    Ok(expr)
                } else {
                    self.ctx
                        .insert_into_values(&id, *rhs, &mut values_defined_here)
                        .await;
                    Ok(Syntax::ValAny())
                }
            }
            Syntax::Asg(lhs, rhs) => {
                if !first {
                    Ok(expr)
                } else {
                    if let Ok((id, syntax)) = extract_id(*lhs.clone(), Vec::new()).await {
                        self.ctx
                            .insert_fns(
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
                        let rhs = self.execute(*rhs, false).await?;
                        if !self
                            .ctx
                            .clone()
                            .set_values_in_context(
                                &mut self.ctx.get_holder(),
                                &lhs.clone(),
                                &rhs,
                                &mut values_defined_here,
                            )
                            .await
                        {
                            self.ctx.remove_values(&mut values_defined_here).await;
                            self.ctx
                                .push_error(
                                    format!("{lhs} must be assignable to {rhs} but is not",),
                                )
                                .await;
                        } else {
                            values_defined_here.clear();
                        }
                    }

                    Ok(Syntax::ValAny())
                }
            }
            Syntax::Let((lhs, rhs), expr) => {
                if no_change {
                    // no changes happen in the RHS of the assignment
                    // => evaulate the let expression
                    if !local_ctx
                        .set_values_in_context(
                            &mut self.ctx.get_holder(),
                            &lhs,
                            &rhs,
                            &mut values_defined_here,
                        )
                        .await
                    {
                        local_ctx.remove_values(&mut values_defined_here);

                        let new_rhs = self.execute_once(*rhs.clone(), first, no_change).await?;
                        if *rhs == new_rhs {
                            self.ctx
                                .push_error(format!("Let expression failed: {expr}"))
                                .await;

                            Err(InterpreterError::LetDoesMatch(format!(
                                "{lhs} = {rhs} does not match"
                            )))
                        } else {
                            Ok(Syntax::Let((lhs, Box::new(new_rhs)), expr))
                        }
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
                    Ok(Syntax::Let(
                        (
                            lhs,
                            Box::new(self.execute_once(*rhs, first, no_change).await?),
                        ),
                        expr,
                    ))
                }
            }
            Syntax::BiOp(BiOpType::OpPeriod, box Syntax::Map(mut map), box Syntax::Id(id)) => {
                if let Some((expr, _)) = map.remove(&id) {
                    Ok(expr)
                } else {
                    Ok(expr)
                }
            }
            Syntax::BiOp(
                BiOpType::OpPeriod,
                box Syntax::Context(ctx_id, system_id, ctx_name),
                box Syntax::Id(id),
            ) => {
                let mut lhs_ctx = self.ctx.get_holder().get(ctx_id).await.unwrap();

                if let Some(global) = lhs_ctx.get_global(&id).await {
                    Ok(Syntax::Contextual(ctx_id, system_id, Box::new(global)))
                } else {
                    Err(InterpreterError::GlobalNotInContext(ctx_name, id))
                }
            }
            Syntax::BiOp(BiOpType::OpPeriod, lhs, rhs) => {
                let lhs = self.execute_once(*lhs, false, no_change).await?;
                Ok(Syntax::BiOp(BiOpType::OpPeriod, Box::new(lhs), rhs))
            }
            Syntax::BiOp(op @ BiOpType::OpGeq, lhs, rhs)
            | Syntax::BiOp(op @ BiOpType::OpLeq, lhs, rhs)
            | Syntax::BiOp(op @ BiOpType::OpGt, lhs, rhs)
            | Syntax::BiOp(op @ BiOpType::OpLt, lhs, rhs) => {
                let lhs = self.execute_once(*lhs, false, no_change).await?;
                let rhs = self.execute_once(*rhs, false, no_change).await?;

                Ok(Syntax::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Syntax::BiOp(op @ BiOpType::OpEq, lhs, rhs)
            | Syntax::BiOp(op @ BiOpType::OpNeq, lhs, rhs) => {
                if no_change {
                    let old_lhs = lhs.clone();
                    let old_rhs = rhs.clone();
                    let lhs = self.execute_once(*lhs, false, true).await?;
                    let rhs = self.execute_once(*rhs, false, true).await?;

                    if lhs == *old_lhs && rhs == *old_rhs {
                        if op == BiOpType::OpEq {
                            Ok(if lhs.eval_equal(&rhs).await {
                                Syntax::ValAtom("true".to_string())
                            } else {
                                Syntax::ValAtom("false".to_string())
                            })
                        } else {
                            Ok(if !lhs.eval_equal(&rhs).await {
                                Syntax::ValAtom("true".to_string())
                            } else {
                                Syntax::ValAtom("false".to_string())
                            })
                        }
                    } else {
                        Ok(Syntax::BiOp(op, Box::new(lhs), Box::new(rhs)))
                    }
                } else {
                    Ok(Syntax::BiOp(
                        op,
                        Box::new(self.execute_once(*lhs, false, no_change).await?),
                        Box::new(self.execute_once(*rhs, false, no_change).await?),
                    ))
                }
            }
            Syntax::BiOp(op, lhs, rhs) => {
                let lhs = self.execute_once(*lhs, false, no_change).await?;
                let rhs = self.execute_once(*rhs, false, no_change).await?;

                Ok(Syntax::BiOp(op, Box::new(lhs), Box::new(rhs)))
            }
            Syntax::If(cond, expr_true, expr_false) => {
                if no_change {
                    // the condition doesn't change anymore
                    // => try to evaluate the condition
                    Ok(match *cond {
                        Syntax::ValAtom(id) if id == "true" => *expr_true,
                        Syntax::ValAtom(id) if id == "false" => *expr_false,
                        _ => {
                            // That failed => try to evaluate the expression even more
                            let old_cond = cond.clone();
                            let cond = self.execute_once(*cond, false, true).await?;

                            if cond == *old_cond {
                                self.ctx
                                    .push_error(
                                        "Expected :true or :false in if-condition".to_string(),
                                    )
                                    .await;
                                expr
                            } else {
                                Syntax::If(Box::new(cond), expr_true, expr_false)
                            }
                        }
                    })
                } else {
                    Ok(Syntax::If(
                        Box::new(self.execute_once(*cond, false, false).await?),
                        expr_true,
                        expr_false,
                    ))
                }
            }
            Syntax::Tuple(lhs, rhs) => {
                let lhs = self.execute_once(*lhs, false, no_change).await?;
                let rhs = self.execute_once(*rhs, false, no_change).await?;

                Ok(Syntax::Tuple(Box::new(lhs), Box::new(rhs)))
            }
            Syntax::UnexpectedArguments() => {
                self.ctx
                    .push_error("Unexpected arguments".to_string())
                    .await;
                Ok(expr)
            }
            Syntax::Lst(lst) => {
                let mut result = Vec::with_capacity(lst.len());
                let mut should_execute = true;
                for e in lst {
                    if should_execute {
                        let previous = e.clone();
                        let executed = self.execute_once(e, false, no_change).await?;
                        let changed = previous != executed;

                        result.push(executed);

                        if changed {
                            should_execute = false;
                            break;
                        }
                    } else {
                        result.push(e);
                    }
                }

                Ok(Syntax::Lst(result))
            }
            Syntax::LstMatch(lst) => {
                let mut result = Vec::new();
                let mut should_execute = true;
                for e in lst {
                    if should_execute {
                        let previous = e.clone();
                        let executed = self.execute_once(e, false, no_change).await?;
                        let changed = previous != executed;

                        result.push(executed);

                        if changed {
                            should_execute = false;
                            break;
                        }
                    } else {
                        result.push(e);
                    }
                }

                Ok(Syntax::LstMatch(result))
            }
            Syntax::Map(map) => {
                let map = join_all(map.into_iter().map({
                    let ctx = self.ctx.clone();
                    move |(key, (val, is_id))| {
                        let mut ctx = ctx.clone();
                        let mut system = self.system.clone();

                        async move {
                            let mut executor = Executor::new(&mut ctx, &mut system);
                            Ok((
                                key,
                                (executor.execute_once(val, false, no_change).await?, is_id),
                            ))
                        }
                    }
                }))
                .await
                .into_iter()
                .try_collect()?;

                Ok(Syntax::Map(map))
            }
            Syntax::MapMatch(_) => Ok(expr),
            Syntax::ExplicitExpr(box expr) => Ok(expr),
            Syntax::Contextual(ctx_id, system_id, expr)
                if ctx_id == self.ctx.get_id() && system_id == self.system.get_id() =>
            {
                Ok(*expr)
            }
            Syntax::Contextual(ctx_id, system_id, expr) => {
                let holder = self.ctx.get_holder();
                let mut ctx = holder.clone().get_handler(ctx_id).await.unwrap();
                let mut executor = Executor::new(&mut ctx, self.system);
                Ok(Syntax::Contextual(
                    ctx_id,
                    system_id,
                    Box::new(executor.execute_once(*expr, first, no_change).await?),
                ))
            }
            expr @ Syntax::Context(_, _, _) => Ok(expr),
            expr @ Syntax::Signal(_, _) => Ok(expr),
            expr @ Syntax::FnOp(_) => Ok(expr),
        }?;

        Ok(expr)
    }

    /// Execute the AST until no longer possible
    #[async_recursion]
    pub async fn execute(&mut self, expr: Syntax, first: bool) -> Result<Syntax, InterpreterError> {
        let mut expr = expr;
        let mut old = expr.clone();
        let mut haschanged = true;
        loop {
            expr = self
                .execute_once(expr.reduce().await, first, !haschanged)
                .await?;

            // Execute as long there are changes
            if expr == old {
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

            if first && expr != Syntax::ValAny() && haschanged {
                debug!("{}", expr);
            }

            old = expr.clone();
        }

        debug!("{:?}", expr);
        Ok(expr)
    }
}

/// Depending on the code, this creates a new system (depending on the current system) or copies
/// the old system.
async fn build_system(
    ctx: ContextHandler,
    mut system: SystemHandler,
    builtins_map: BTreeMap<String, Syntax>,
) -> SystemHandler {
    let has_restrict = builtins_map.get("restrict") == Some(&Syntax::ValAtom("true".to_string()));
    debug!("has_restrict: {has_restrict}");

    let has_restrict_insecure =
        builtins_map.get("restrict_insecure") == Some(&Syntax::ValAtom("true".to_string()));
    debug!("has_restrict_insecure: {has_restrict_insecure}");

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

    if builtins_map.is_empty() && !has_restrict {
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
                new_builtins_map.entry(syscall_type).or_insert_with(|| {
                    debug!("{syscall_type:?}");
                    Syntax::ValAtom("error".to_string())
                });
            }
        } else if has_restrict_insecure {
            for syscall_type in all_syscalls
                .into_iter()
                .filter(|syscall| syscall.is_secure())
            {
                new_builtins_map.entry(syscall_type).or_insert_with(|| {
                    debug!("{syscall_type:?}");
                    Syntax::ValAtom("error".to_string())
                });
            }
        }

        system
            .get_holder()
            .new_system_handler(new_builtins_map)
            .await
    }
}

#[async_recursion]
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
            extract_id(*lhs.clone(), unpacked).await
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

const BUILTIN_MODULES: &[&'static str] = &["std", "sys", "str"];

/// Imports the specified library (including builtin libraries)
async fn import_lib(
    ctx: &mut ContextHandler,
    system: &mut SystemHandler,
    path: String,
    builtins_map: BTreeMap<String, Syntax>,
) -> Result<Syntax, InterpreterError> {
    let system = {
        let ctx = ctx.clone();
        let system = system.clone();

        let handle = tokio::spawn(async { build_system(ctx, system, builtins_map).await });

        async {
            handle
                .await
                .map_err(|err| InterpreterError::InternalError(err.to_string()))
        }
    };

    if let Some(ctx) = ctx.get_holder().get_path(&path).await {
        Ok(Syntax::Context(ctx.get_id(), system.await?.get_id(), path))
    } else if
    /* List all builtin libraries here -> */
    BUILTIN_MODULES.iter().any({
        let path = path.clone();
        move |&p| p == path
    }) {
        import_std_lib(ctx, &mut system.await?, path).await
    } else if let Some(ctx_path) = ctx.get_path().await {
        let mut module_path = ctx_path.join(path.clone());
        // Normalize the path
        module_path = std::fs::canonicalize(module_path.clone()).unwrap_or(module_path.clone());

        let module_path_str = module_path.to_string_lossy().to_string();
        if module_path.is_file() {
            if let Some(ctx) = ctx.get_holder().get_path(&module_path_str).await {
                Ok(Syntax::Context(
                    ctx.get_id(),
                    system.await?.get_id(),
                    module_path_str,
                ))
            } else {
                let mut system = system.await?;
                let code = std::fs::read_to_string(&module_path).map_err({
                    let module_path_str = module_path_str.clone();
                    move |_| InterpreterError::ImportFileDoesNotExist(module_path_str)
                })?;

                let holder = &mut ctx.get_holder();

                let ctx = execute_code(
                    &module_path_str,
                    module_path.parent().map(|path| path.to_path_buf()),
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
        } else {
            ctx.push_error(format!("Expected {module_path_str} to be a file"))
                .await;

            Err(InterpreterError::ImportFileDoesNotExist(module_path_str))
        }
    } else {
        ctx.push_error("Import can only be done in real modules".to_string())
            .await;

        let result = import_std_lib(ctx, &mut system.await?, path).await;
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
                    let val = val.reduce_all().await;
                    ctx.replace_from_values(&id, val).await;

                    if ctx.add_global(&id, values_defined_here).await {
                        return Ok(Syntax::ValAny());
                    }
                }

                ctx.push_error(format!("Cannot export symbol {id}")).await;

                Ok(Syntax::Call(
                    Box::new(Syntax::Id("export".to_string())),
                    Box::new(Syntax::Id(id)),
                ))
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
            ("syscall", Syntax::Tuple(box Syntax::ValAtom(id), expr)) => {
                let syscall: Result<SystemCallType, _> = id.as_str().try_into();

                match syscall {
                    Ok(syscall) => {
                        system
                            .clone()
                            .do_syscall(ctx, system, no_change, syscall, *expr)
                            .await
                    }
                    Err(_) => Ok(original_expr),
                }
            }
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
            include_str!("../modules/std.pgcl")
        } else if path == "sys" {
            include_str!("../modules/sys.pgcl")
        } else if path == "str" {
            include_str!("../modules/str.pgcl")
        } else {
            ctx.push_error(format!("Expected {path} to be a file"))
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
    name: &str,
    path: Option<PathBuf>,
    code: &str,
    holder: &mut ContextHolder,
    system: &mut SystemHandler,
) -> Result<ContextHandler, InterpreterError> {
    let mut ctx = holder
        .create_context(name.to_owned(), path)
        .await
        .handler(holder.clone());
    holder
        .set_path(name, &holder.get(ctx.get_id()).await.unwrap())
        .await;

    let toks: Vec<(SyntaxKind, String)> = Token::lex_for_rowan(code)
        .into_iter()
        .map(
            |(tok, slice)| -> Result<(SyntaxKind, String), InterpreterError> {
                Ok((tok.try_into()?, slice))
            },
        )
        .try_collect()?;

    let typed = parse_to_typed(toks);
    debug!("{:?}", typed);
    let typed = typed?;
    Executor::new(&mut ctx, system).execute(typed, true).await?;

    Ok(ctx)
}

fn parse_to_typed(toks: Vec<(SyntaxKind, String)>) -> Result<Syntax, InterpreterError> {
    let (ast, errors) = Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
    //print_ast(0, &ast);
    if !errors.is_empty() {
        eprintln!("{errors:?}");
    }

    (*ast).try_into()
}
