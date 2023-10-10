use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
};

use async_recursion::async_recursion;
use futures::future::join_all;
use regex::Regex;
use tokio::sync::Mutex;

use crate::{execute::{Syntax, BiOpType}, gc::mark_used, system::SystemHandler};

pub enum PrivateContextMark {
    OddMark,
    EvenMark,
}

pub struct PrivateContext {
    id: usize,
    name: String,
    path: Option<PathBuf>,
    /// HashMap with a stack of values
    values: HashMap<String, Vec<Syntax>>,
    fns: HashMap<String, Vec<(Vec<Syntax>, Syntax)>>,
    errors: Vec<String>,
    globals: HashMap<String, Syntax>,
    marked: PrivateContextMark,
}

impl PrivateContext {
    pub fn new(id: usize, name: String, path: Option<PathBuf>) -> Self {
        Self {
            id,
            name,
            path,
            values: Default::default(),
            fns: Default::default(),
            errors: Default::default(),
            globals: Default::default(),
            marked: PrivateContextMark::OddMark, // unmarked
        }
    }

    pub fn get_id(&self) -> usize {
        self.id
    }

    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn get_path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    pub async fn add_global(&mut self, id: &String, values_defined_here: &mut Vec<String>) -> bool {
        if self.globals.contains_key(id) {
            return false;
        }

        if let Some(expr) = self.get_from_values(id, values_defined_here).await {
            self.globals.insert(id.clone(), expr);

            true
        } else {
            false
        }
    }

    pub async fn get_global(&mut self, id: &String) -> Option<Syntax> {
        self.globals.get(id).cloned()
    }

    pub async fn get_globals(&mut self) -> Vec<Syntax> {
        self.globals.values().cloned().collect()
    }

    pub async fn get_values(&mut self) -> Vec<Syntax> {
        self.values.values().flatten().cloned().collect()
    }

    pub fn insert_into_values(
        &mut self,
        id: &String,
        syntax: Syntax,
        values_defined_here: &mut Vec<String>,
    ) {
        let stack = if let Some(stack) = self.values.get_mut(id) {
            stack
        } else {
            self.values.insert(id.clone(), Default::default());
            self.values.get_mut(id).unwrap()
        };

        stack.push(syntax);
        values_defined_here.push(id.clone());
    }

    pub fn get_errors(&self) -> Vec<String> {
        self.errors.clone()
    }

    pub fn push_error(&mut self, err: String) {
        self.errors.push(err);
    }

    pub fn insert_fns(&mut self, id: String, syntax: (Vec<Syntax>, Syntax)) {
        if let Some(stack) = self.fns.get_mut(&id) {
            stack.push(syntax);
        } else {
            self.fns.insert(id.clone(), Default::default());
            let stack = self.fns.get_mut(&id).unwrap();
            stack.push(syntax);
        }
    }

    pub fn remove_values(
        &mut self,
        values_defined_here: &mut Vec<String>,
    ) -> HashMap<String, Syntax> {
        let mut result = HashMap::with_capacity(values_defined_here.len());

        for id in values_defined_here.iter() {
            if let Some(stack) = self.values.get_mut(id) {
                if let Some(value) = stack.pop() {
                    if !result.contains_key(id) {
                        result.insert(id.clone(), value);
                    }
                }
            }
        }

        values_defined_here.clear();

        result
    }

    pub async fn replace_from_values(&mut self, id: &String, syntax: Syntax) -> bool {
        if let Some(stack) = self.values.get_mut(id) {
            if stack.pop().is_some() {
                stack.push(syntax);

                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub async fn get_from_values(
        &mut self,
        id: &String,
        values_defined_here: &mut Vec<String>,
    ) -> Option<Syntax> {
        if let Some(stack) = self.values.get(id) {
            stack.last().cloned()
        } else if let Some(fns) = self.fns.get(id) {
            let compiled_fn = build_fn(id, fns).await;
            self.insert_into_values(id, compiled_fn.clone(), values_defined_here);
            Some(compiled_fn)
        } else {
            None
        }
    }

    pub async fn mark(&mut self, system: &mut SystemHandler, even: bool) {
        match self.marked {
            PrivateContextMark::OddMark if !even => {
                return;
            }
            PrivateContextMark::EvenMark if even => {
                return;
            }
            _ => {}
        }

        self.marked = match even {
            true => PrivateContextMark::EvenMark,
            false => PrivateContextMark::OddMark,
        };

        mark_used(
            &mut *system,
            &self
                .globals
                .values()
                .into_iter()
                .chain(self.values.values().into_iter().flatten())
                .collect::<Vec<&Syntax>>(),
        )
        .await;
    }

    pub async fn set_values_in_context(
        &mut self,
        holder: &mut ContextHolder,
        lhs: &Syntax,
        rhs: &Syntax,
        values_defined_here: &mut Vec<String>,
    ) -> bool {
        set_values_in_context(self, holder, lhs, rhs, values_defined_here).await
    }
}

enum ComparisonResult {
    Continue(Vec<(Syntax, Syntax)>),
    Matched(bool),
}

async fn build_fn(name: &str, fns: &[(Vec<Syntax>, Syntax)]) -> Syntax {
    let args_len = fns[0].0.len();
    let arg_name = {
        let mut dont_use_varnames: HashSet<&String> = HashSet::new();
        for i in 0..fns.len() {
            let params_vars: HashSet<&String> = join_all(
                fns[i]
                    .0
                    .iter()
                    .map(|param| async move { param.get_args().await }),
            )
            .await
            .into_iter()
            .fold(HashSet::new(), |mut result, part| {
                result.extend(part.into_iter());
                result
            });

            dont_use_varnames.extend(fns[i].1.get_args().await.difference(&params_vars));
        }

        let mut params_vars: Vec<HashSet<&String>> =
            (0..args_len).map(|_| HashSet::new()).collect();
        for i in 0..args_len {
            for j in 0..fns.len() {
                params_vars[i].extend(fns[j].0[i].get_args().await.into_iter());
            }
        }

        let use_param_names: Vec<Option<String>> = params_vars
            .into_iter()
            .map(|mut names| {
                let mut names: Vec<&String> = names.drain().collect();
                names.sort_unstable();

                names.into_iter().next()
            })
            .map(move |name| match name {
                Some(name) => {
                    if dont_use_varnames.contains(name) {
                        None
                    } else {
                        Some(name.to_owned())
                    }
                }
                None => None,
            })
            .collect();

        let name = name.to_owned();
        move |arg: usize| {
            if let Some(param) = use_param_names.get(arg).unwrap_or(&None) {
                param.to_owned()
            } else {
                format!("{}{}_{}", name.len(), name, arg)
            }
        }
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

    let params: Vec<String> = (0..args_len).map(arg_name).collect();

    let mut fns: Vec<(Vec<Syntax>, Syntax)> = fns.to_vec();
    fns.reverse();

    build_fn_with_args(&params, &params, &fns).reduce().await
}

impl PartialEq for PrivateContext {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl From<PrivateContext> for Context {
    fn from(val: PrivateContext) -> Self {
        Context {
            id: val.id,
            ctx: Arc::new(Mutex::new(val)),
        }
    }
}

#[derive(Clone)]
pub struct Context {
    id: usize,
    ctx: Arc<Mutex<PrivateContext>>,
}

impl Context {
    pub fn get_id(&self) -> usize {
        self.id
    }

    pub async fn get_path(&self) -> Option<PathBuf> {
        self.ctx.lock().await.get_path()
    }

    pub async fn get_name(&self) -> String {
        self.ctx.lock().await.get_name()
    }

    pub async fn add_global(&mut self, id: &String, values_defined_here: &mut Vec<String>) -> bool {
        self.ctx
            .lock()
            .await
            .add_global(id, values_defined_here)
            .await
    }

    pub async fn get_global(&mut self, id: &String) -> Option<Syntax> {
        self.ctx.lock().await.get_global(id).await
    }

    pub async fn get_globals(&mut self) -> Vec<Syntax> {
        self.ctx.lock().await.get_globals().await
    }

    pub async fn get_values(&mut self) -> Vec<Syntax> {
        self.ctx.lock().await.get_values().await
    }

    pub async fn mark(&mut self, system: &mut SystemHandler, even: bool) {
        self.ctx.lock().await.mark(system, even).await
    }

    pub async fn insert_into_values(
        &mut self,
        id: &String,
        syntax: Syntax,
        values_defined_here: &mut Vec<String>,
    ) {
        self.ctx
            .lock()
            .await
            .insert_into_values(id, syntax, values_defined_here)
    }

    pub async fn replace_from_values(&mut self, id: &String, syntax: Syntax) -> bool {
        self.ctx.lock().await.replace_from_values(id, syntax).await
    }

    pub async fn get_errors(&self) -> Vec<String> {
        self.ctx.lock().await.get_errors()
    }

    pub async fn push_error(&mut self, err: String) {
        self.ctx.lock().await.push_error(err)
    }

    pub async fn insert_fns(&mut self, id: String, syntax: (Vec<Syntax>, Syntax)) {
        self.ctx.lock().await.insert_fns(id, syntax)
    }

    pub async fn remove_values(
        &mut self,
        values_defined_here: &mut Vec<String>,
    ) -> HashMap<String, Syntax> {
        self.ctx.lock().await.remove_values(values_defined_here)
    }

    pub async fn get_from_values(
        &mut self,
        id: &String,
        values_defined_here: &mut Vec<String>,
    ) -> Option<Syntax> {
        self.ctx
            .lock()
            .await
            .get_from_values(id, values_defined_here)
            .await
    }

    pub fn handler(&self, holder: ContextHolder) -> ContextHandler {
        ContextHandler {
            id: self.get_id(),
            holder,
        }
    }

    pub async fn set_values_in_context(
        &mut self,
        holder: &mut ContextHolder,
        lhs: &Syntax,
        rhs: &Syntax,
        values_defined_here: &mut Vec<String>,
    ) -> bool {
        self.ctx
            .lock()
            .await
            .set_values_in_context(holder, lhs, rhs, values_defined_here)
            .await
    }
}

#[derive(Default, Clone)]
struct PrivateContextHandler {
    id_to_ctx: BTreeMap<usize, Context>,
    last_id: usize,
    path_to_ctx: BTreeMap<String, Context>,
}

impl PrivateContextHandler {
    pub fn get(&self, id: usize) -> Option<Context> {
        self.id_to_ctx.get(&id).cloned()
    }

    pub fn create_context(&mut self, name: String, path: Option<PathBuf>) -> Context {
        let id = self.last_id;
        let ctx: Context = PrivateContext::new(id, name, path).into();
        self.last_id += 1;

        self.id_to_ctx.insert(id, ctx.clone());

        ctx
    }

    pub fn set_path(&mut self, name: &str, ctx: &Context) {
        self.path_to_ctx.insert(name.to_owned(), ctx.clone());
    }

    pub fn get_path(&mut self, name: &String) -> Option<Context> {
        self.path_to_ctx.get(name).cloned()
    }

    pub async fn mark(&mut self, system: &mut SystemHandler, even: bool) {
        join_all(self.id_to_ctx.values_mut().map(|ctx| {
            let mut system = system.clone();

            async move {
                ctx.mark(&mut system, even).await;
            }
        }))
        .await;
    }
}

#[derive(Clone)]
pub struct ContextHandler {
    id: usize,
    holder: ContextHolder,
}

impl ContextHandler {
    pub async fn async_default() -> Self {
        let mut holder = ContextHolder::default();
        let ctx = holder.create_context("<stdin>".to_string(), None).await;

        ctx.handler(holder)
    }
}

impl ContextHandler {
    pub fn new(id: usize, ctx_holder: ContextHolder) -> Self {
        Self {
            id,
            holder: ctx_holder,
        }
    }

    pub fn get_id(&self) -> usize {
        self.id
    }

    pub async fn get_name(&self) -> String {
        self.holder.get(self.id).await.unwrap().get_name().await
    }

    pub async fn get_path(&self) -> Option<PathBuf> {
        self.holder.get(self.id).await.unwrap().get_path().await
    }

    pub async fn add_global(&mut self, id: &String, values_defined_here: &mut Vec<String>) -> bool {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .add_global(id, values_defined_here)
            .await
    }

    pub async fn get_global(&mut self, id: &String) -> Option<Syntax> {
        self.holder.get(self.id).await.unwrap().get_global(id).await
    }

    pub async fn get_globals(&mut self) -> Vec<Syntax> {
        self.holder.get(self.id).await.unwrap().get_globals().await
    }

    pub async fn get_values(&mut self) -> Vec<Syntax> {
        self.holder.get(self.id).await.unwrap().get_values().await
    }

    pub async fn mark(&mut self, system: &mut SystemHandler, even: bool) {
        self.holder.mark(system, even).await
    }

    pub async fn insert_into_values(
        &mut self,
        id: &String,
        syntax: Syntax,
        values_defined_here: &mut Vec<String>,
    ) {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .insert_into_values(id, syntax, values_defined_here)
            .await
    }

    pub async fn replace_from_values(&mut self, id: &String, syntax: Syntax) -> bool {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .replace_from_values(id, syntax)
            .await
    }

    pub async fn get_errors(&self) -> Vec<String> {
        self.holder.get(self.id).await.unwrap().get_errors().await
    }

    pub async fn push_error(&mut self, err: String) {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .push_error(err)
            .await
    }

    pub async fn insert_fns(&mut self, id: String, syntax: (Vec<Syntax>, Syntax)) {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .insert_fns(id, syntax)
            .await
    }

    pub async fn remove_values(
        &mut self,
        values_defined_here: &mut Vec<String>,
    ) -> HashMap<String, Syntax> {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .remove_values(values_defined_here)
            .await
    }

    pub async fn get_from_values(
        &mut self,
        id: &String,
        values_defined_here: &mut Vec<String>,
    ) -> Option<Syntax> {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .get_from_values(id, values_defined_here)
            .await
    }

    pub fn get_holder(&self) -> ContextHolder {
        self.holder.clone()
    }

    pub async fn set_values_in_context(
        &mut self,
        lhs: &Syntax,
        rhs: &Syntax,
        values_defined_here: &mut Vec<String>,
    ) -> bool {
        self.holder
            .get(self.get_id())
            .await
            .unwrap()
            .ctx
            .lock()
            .await
            .set_values_in_context(&mut self.get_holder(), lhs, rhs, values_defined_here)
            .await
    }
}

#[derive(Clone, Default)]
pub struct ContextHolder {
    handler: Arc<Mutex<PrivateContextHandler>>,
}

impl ContextHolder {
    pub async fn get(&self, id: usize) -> Option<Context> {
        self.handler.lock().await.get(id)
    }

    pub async fn get_handler(&self, id: usize) -> Option<ContextHandler> {
        if self.handler.lock().await.get(id).is_some() {
            Some(ContextHandler {
                id,
                holder: self.clone(),
            })
        } else {
            None
        }
    }

    pub async fn create_context(&mut self, name: String, path: Option<PathBuf>) -> Context {
        self.handler.lock().await.create_context(name, path)
    }

    pub async fn set_path(&mut self, name: &str, ctx: &Context) {
        self.handler.lock().await.set_path(name, ctx);
    }

    pub async fn get_path(&mut self, name: &String) -> Option<Context> {
        self.handler.lock().await.get_path(name)
    }

    pub async fn mark(&mut self, system: &mut SystemHandler, even: bool) {
        self.handler.lock().await.mark(system, even).await;
    }
}

#[async_recursion]
pub async fn set_values_in_context(
    ctx: &mut PrivateContext,
    holder: &mut ContextHolder,
    lhs: &Syntax,
    rhs: &Syntax,
    values_defined_here: &mut Vec<String>,
) -> bool {
    let mut comparisons: VecDeque<(Syntax, Syntax)> = Default::default();
    comparisons.push_front((lhs.clone(), rhs.clone()));
    while let Some((lhs, rhs)) = comparisons.pop_front() {
        match set_values_in_context_one(ctx, holder, &lhs, &rhs, values_defined_here).await {
            ComparisonResult::Matched(result) => {
                if !result {
                    return false;
                }
            }
            ComparisonResult::Continue(result) => {
                comparisons.extend(result.into_iter());
            }
        }
    }

    true
}

pub async fn set_values_in_context_one(
    ctx: &mut PrivateContext,
    holder: &mut ContextHolder,
    lhs: &Syntax,
    rhs: &Syntax,
    values_defined_here: &mut Vec<String>,
) -> ComparisonResult {
    match (lhs, rhs) {
        (Syntax::Tuple(a0, b0), Syntax::Tuple(a1, b1)) => {
            ComparisonResult::Continue(vec![(*a0.clone(), *a1.clone()), (*b0.clone(), *b1.clone())])
        }
        (Syntax::Call(a0, b0), Syntax::Call(a1, b1)) => {
            ComparisonResult::Continue(vec![(*a0.clone(), *a1.clone()), (*b0.clone(), *b1.clone())])
        }
        (Syntax::ValAny(), _) => ComparisonResult::Matched(true),
        (Syntax::Id(id), expr) => {
            ctx.insert_into_values(id, expr.clone(), values_defined_here);
            ComparisonResult::Matched(true)
        }
        (Syntax::BiOp(op0, a0, b0), Syntax::BiOp(op1, a1, b1)) => {
            if op0 != op1 {
                ComparisonResult::Matched(false)
            } else {
                ComparisonResult::Continue(vec![
                    (*a0.clone(), *a1.clone()),
                    (*b0.clone(), *b1.clone()),
                ])
            }
        }
        (Syntax::Lst(lst0), Syntax::Lst(lst1)) if lst0.len() == lst1.len() => {
            ComparisonResult::Continue(
                lst0.iter()
                    .zip(lst1.iter())
                    .map(|(lhs, rhs)| (lhs.clone(), rhs.clone()))
                    .collect(),
            )
        }
        (Syntax::Lst(lst0), Syntax::ValStr(str1)) if lst0.len() == str1.chars().count() => {
            let str1: Vec<char> = str1.chars().collect();
            for i in 0..lst0.len() {
                let str1 = Syntax::ValStr(format!("{}", str1[i]));
                if !set_values_in_context(ctx, holder, &lst0[i], &str1, values_defined_here).await {
                    return ComparisonResult::Matched(false);
                }
            }

            ComparisonResult::Matched(true)
        }
        (Syntax::LstMatch(lst0), Syntax::Lst(lst1))
            if lst0.len() >= 2 && lst1.len() + 1 >= lst0.len() =>
        {
            let mut result = Vec::new();
            let mut lst1: &[Syntax] = lst1;
            for i in 0..lst0.len() {
                if i == lst0.len() - 1 {
                    let lst1 = Syntax::Lst(lst1.into());
                    result.push((lst0[i].clone(), lst1));
                    break;
                } else {
                    let lst1_val = lst1[0].clone();
                    lst1 = &lst1[1..];
                    result.push((lst0[i].clone(), lst1_val));
                }
            }

            ComparisonResult::Continue(result)
        }
        (Syntax::LstMatch(lst0), Syntax::BiOp(BiOpType::OpAdd, box Syntax::Lst(lst1), expr)) if lst0.len() + 1 == lst1.len() => {
            let mut result = Vec::new();
            let mut lst1: &[Syntax] = lst1;
            for i in 0..(lst0.len()-1) {
                if i == lst0.len() - 1 {
                    let lst1 = Syntax::Lst(lst1.into());
                    result.push((lst0[i].clone(), lst1));
                    break;
                } else {
                    let lst1_val = lst1[0].clone();
                    lst1 = &lst1[1..];
                    result.push((lst0[i].clone(), lst1_val));
                }
            }

            result.push((lst0[lst0.len() - 1].clone(), *expr.clone()));

            ComparisonResult::Continue(result)
        }
        (Syntax::LstMatch(lst0), Syntax::ValStr(str1))
            if lst0.len() >= 2 && str1.chars().count() + 1 >= lst0.len() =>
        {
            let mut str1: Vec<char> = str1.chars().collect();
            let end_idx = lst0.len() - 1;
            for (i, expr) in lst0.iter().enumerate() {
                if i == end_idx {
                    let str1: String = str1.into_iter().collect();
                    let str1 = Syntax::ValStr(str1);
                    if !set_values_in_context(ctx, holder, expr, &str1, values_defined_here).await {
                        return ComparisonResult::Matched(false);
                    }

                    break;
                } else {
                    if str1.is_empty() {
                        return ComparisonResult::Matched(false);
                    }

                    let len = match &expr {
                        Syntax::ValStr(s) => {
                            let len = s.len();
                            if str1.len() < len {
                                return ComparisonResult::Matched(false);
                            }

                            len
                        }
                        _ => 1,
                    };

                    let c: String = str1.drain(0..len).collect();
                    let expr_str1 = Syntax::ValStr(c);

                    if !set_values_in_context(ctx, holder, expr, &expr_str1, values_defined_here)
                        .await
                    {
                        return ComparisonResult::Matched(false);
                    }
                }
            }

            ComparisonResult::Matched(true)
        }
        (Syntax::Map(lhs), Syntax::Map(rhs)) => {
            for (key, (val, is_id)) in lhs.iter() {
                if let Some((rhs, _)) = rhs.get(key) {
                    if *is_id {
                        set_values_in_context(
                            ctx,
                            holder,
                            &Syntax::Id(key.clone()),
                            rhs,
                            values_defined_here,
                        )
                        .await;
                    }

                    if !set_values_in_context(ctx, holder, val, rhs, values_defined_here).await {
                        return ComparisonResult::Matched(false);
                    }
                } else {
                    return ComparisonResult::Matched(false);
                }
            }

            ComparisonResult::Matched(true)
        }
        (Syntax::MapMatch(lhs), Syntax::Map(rhs)) => {
            for (key, key_into, val, is_id) in lhs.iter() {
                if let Some((rhs, _)) = rhs.get(key) {
                    if *is_id {
                        set_values_in_context(
                            ctx,
                            holder,
                            &Syntax::Id(key.clone()),
                            rhs,
                            values_defined_here,
                        )
                        .await;
                    } else if let Some(key) = key_into {
                        set_values_in_context(
                            ctx,
                            holder,
                            &Syntax::Id(key.clone()),
                            rhs,
                            values_defined_here,
                        )
                        .await;
                    }

                    if let Some(val) = val {
                        if !set_values_in_context(ctx, holder, val, rhs, values_defined_here).await
                        {
                            return ComparisonResult::Matched(false);
                        }
                    }
                } else {
                    return ComparisonResult::Matched(false);
                }
            }

            ComparisonResult::Matched(true)
        }
        (Syntax::MapMatch(lhs), Syntax::Context(ctx_id, system_id, _)) => {
            let private_ctx = ctx;
            let mut ctx = holder.get(*ctx_id).await.unwrap();

            for (key, key_into, val, is_id) in lhs.iter() {
                if let Some(rhs) = ctx.get_global(key).await {
                    if *is_id {
                        PrivateContext::set_values_in_context(
                            private_ctx,
                            holder,
                            &Syntax::Id(key.clone()),
                            &Syntax::Contextual(*ctx_id, *system_id, Box::new(rhs.clone())),
                            values_defined_here,
                        )
                        .await;
                    } else if let Some(key) = key_into {
                        PrivateContext::set_values_in_context(
                            private_ctx,
                            holder,
                            &Syntax::Id(key.clone()),
                            &Syntax::Contextual(*ctx_id, *system_id, Box::new(rhs.clone())),
                            values_defined_here,
                        )
                        .await;
                    }

                    if let Some(val) = val {
                        if !PrivateContext::set_values_in_context(
                            private_ctx,
                            holder,
                            val,
                            &rhs,
                            values_defined_here,
                        )
                        .await
                        {
                            return ComparisonResult::Matched(false);
                        }
                    }
                } else {
                    return ComparisonResult::Matched(false);
                }
            }

            ComparisonResult::Matched(true)
        }
        (Syntax::ValRg(re), Syntax::ValStr(s)) => {
            ComparisonResult::Matched(Regex::new(re).unwrap().is_match(s))
        }
        (Syntax::Call(box Syntax::ValRg(re), rest), Syntax::ValStr(s)) => {
            ComparisonResult::Continue(vec![(
                *rest.clone(),
                eval_regex(&Regex::new(re).unwrap(), s),
            )])
        }
        (Syntax::ExplicitExpr(lhs), rhs) => {
            ComparisonResult::Continue(vec![(*lhs.clone(), rhs.clone())])
        }
        (lhs, Syntax::ExplicitExpr(rhs)) => {
            ComparisonResult::Continue(vec![(lhs.clone(), *rhs.clone())])
        }
        (expr0, expr1) => ComparisonResult::Matched(expr0.eval_equal(expr1).await),
    }
}

fn eval_regex(re: &Regex, s: &str) -> Syntax {
    if let Some(captures) = re.captures(s) {
        Syntax::Tuple(
            Box::new(Syntax::ValAtom("true".to_string())),
            Box::new(Syntax::Lst(
                captures
                    .iter()
                    .map(|capture| match capture {
                        Some(capture) => Syntax::ValStr(capture.as_str().to_string()),
                        None => Syntax::ValAtom("none".to_string()),
                    })
                    .collect(),
            )),
        )
    } else {
        Syntax::Tuple(
            Box::new(Syntax::ValAtom("false".to_string())),
            Box::new(Syntax::Lst(Vec::new())),
        )
    }
}
