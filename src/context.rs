use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

use async_recursion::async_recursion;
use log::debug;
use tokio::sync::Mutex;

use crate::execute::Syntax;

pub struct PrivateContext {
    id: usize,
    name: String,
    path: Option<PathBuf>,
    /// HashMap with a stack of values
    values: HashMap<String, Vec<Syntax>>,
    fns: HashMap<String, Vec<(Vec<Syntax>, Syntax)>>,
    errors: Vec<String>,
    globals: HashMap<String, Syntax>,
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

        if let Some(expr) = self.get_from_values(&id, values_defined_here).await {
            self.globals.insert(id.clone(), expr);

            true
        } else {
            false
        }
    }

    pub async fn get_global(&mut self, id: &String) -> Option<Syntax> {
        self.globals.get(id).map(|expr| expr.clone())
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
            if let Some(_) = stack.pop() {
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
            stack.last().map(|syntax| syntax.clone())
        } else {
            if let Some(fns) = self.fns.get(id) {
                let compiled_fn = build_fn(id, fns).await;
                self.insert_into_values(id, compiled_fn.clone(), values_defined_here);
                Some(compiled_fn)
            } else {
                None
            }
        }
    }

    #[async_recursion]
    pub async fn set_values_in_context(
        &mut self,
        holder: &mut ContextHolder,
        lhs: &Syntax,
        rhs: &Syntax,
        values_defined_here: &mut Vec<String>,
    ) -> bool {
        match (lhs, rhs) {
            (Syntax::Tuple(a0, b0), Syntax::Tuple(a1, b1)) => {
                let a = self
                    .set_values_in_context(holder, a0, a1, values_defined_here)
                    .await;
                let b = self
                    .set_values_in_context(holder, b0, b1, values_defined_here)
                    .await;
                a && b
            }
            (Syntax::Call(a0, b0), Syntax::Call(a1, b1)) => {
                let a = self
                    .set_values_in_context(holder, a0, a1, values_defined_here)
                    .await;
                let b = self
                    .set_values_in_context(holder, b0, b1, values_defined_here)
                    .await;
                a && b
            }
            (Syntax::ValAny(), _) => true,
            (Syntax::Id(id), expr) => {
                self.insert_into_values(id, expr.clone(), values_defined_here);
                true
            }
            (Syntax::BiOp(op0, a0, b0), Syntax::BiOp(op1, a1, b1)) => {
                if op0 != op1 {
                    false
                } else {
                    let a = self
                        .set_values_in_context(holder, a0, a1, values_defined_here)
                        .await;
                    let b = self
                        .set_values_in_context(holder, b0, b1, values_defined_here)
                        .await;
                    a && b
                }
            }
            (Syntax::Lst(lst0), Syntax::Lst(lst1)) if lst0.len() == lst1.len() => {
                for i in 0..lst0.len() {
                    if !self
                        .set_values_in_context(holder, &lst0[i], &lst1[i], values_defined_here)
                        .await
                    {
                        return false;
                    }
                }

                true
            }
            (Syntax::Lst(lst0), Syntax::ValStr(str1)) if lst0.len() == str1.chars().count() => {
                let str1: Vec<char> = str1.chars().collect();
                for i in 0..lst0.len() {
                    let str1 = Syntax::ValStr(format!("{}", str1[i]));
                    if !self
                        .set_values_in_context(holder, &lst0[i], &str1, values_defined_here)
                        .await
                    {
                        return false;
                    }
                }

                true
            }
            (Syntax::LstMatch(lst0), Syntax::Lst(lst1))
                if lst0.len() >= 2 && lst1.len() + 1 > lst0.len() =>
            {
                let mut lst1: &[Syntax] = &lst1;
                for i in 0..lst0.len() {
                    if i == lst0.len() - 1 {
                        let lst1 = Syntax::Lst(lst1.into());
                        if !self
                            .set_values_in_context(holder, &lst0[i], &lst1, values_defined_here)
                            .await
                        {
                            return false;
                        }

                        break;
                    } else {
                        let lst1_val = &lst1[0];
                        lst1 = &lst1[1..];
                        if !self
                            .set_values_in_context(holder, &lst0[i], &lst1_val, values_defined_here)
                            .await
                        {
                            return false;
                        }
                    }
                }

                true
            }
            (Syntax::LstMatch(lst0), Syntax::ValStr(str1))
                if lst0.len() >= 2 && str1.chars().count() + 1 > lst0.len() =>
            {
                let str1: Vec<char> = str1.chars().collect();
                for i in 0..lst0.len() {
                    if i == lst0.len() - 1 {
                        let str1: String = str1[i..].iter().collect();
                        let str1 = Syntax::ValStr(str1);
                        if !self
                            .set_values_in_context(holder, &lst0[i], &str1, values_defined_here)
                            .await
                        {
                            return false;
                        }

                        break;
                    } else {
                        let str1 = Syntax::ValStr(format!("{}", str1[i]));
                        if !self
                            .set_values_in_context(holder, &lst0[i], &str1, values_defined_here)
                            .await
                        {
                            return false;
                        }
                    }
                }

                true
            }
            (Syntax::LstMatch(lst0), Syntax::Lst(lst1))
                if lst0.len() >= 2 && lst1.len() + 1 == lst0.len() =>
            {
                let mut lst1 = lst1.clone();
                for i in (0..lst0.len() - 1).rev() {
                    if !self
                        .set_values_in_context(
                            holder,
                            &lst0[i],
                            &lst1.pop().unwrap(),
                            values_defined_here,
                        )
                        .await
                    {
                        return false;
                    }
                }

                self.set_values_in_context(
                    holder,
                    &lst0.last().unwrap(),
                    &Syntax::Lst(Vec::default()),
                    values_defined_here,
                )
                .await
            }
            (Syntax::LstMatch(lst0), Syntax::ValStr(str1))
                if lst0.len() >= 2 && str1.chars().count() + 1 == lst0.len() =>
            {
                let mut str1: Vec<char> = str1.chars().collect();
                for i in (0..lst0.len() - 1).rev() {
                    if !self
                        .set_values_in_context(
                            holder,
                            &lst0[i],
                            &Syntax::ValStr(format!("{}", str1.pop().unwrap())),
                            values_defined_here,
                        )
                        .await
                    {
                        return false;
                    }
                }

                self.set_values_in_context(
                    holder,
                    &lst0.last().unwrap(),
                    &Syntax::Lst(Vec::default()),
                    values_defined_here,
                )
                .await
            }
            (Syntax::Map(lhs), Syntax::Map(rhs)) => {
                for (key, (val, is_id)) in lhs.iter() {
                    if let Some((rhs, _)) = rhs.get(key) {
                        if *is_id {
                            self.set_values_in_context(
                                holder,
                                &Syntax::Id(key.clone()),
                                rhs,
                                values_defined_here,
                            )
                            .await;
                        }

                        if !self
                            .set_values_in_context(holder, &val, &rhs, values_defined_here)
                            .await
                        {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }

                true
            }
            (Syntax::MapMatch(lhs), Syntax::Map(rhs)) => {
                for (key, key_into, val, is_id) in lhs.iter() {
                    if let Some((rhs, _)) = rhs.get(key) {
                        if *is_id {
                            self.set_values_in_context(
                                holder,
                                &Syntax::Id(key.clone()),
                                rhs,
                                values_defined_here,
                            )
                            .await;
                        } else if let Some(key) = key_into {
                            self.set_values_in_context(
                                holder,
                                &Syntax::Id(key.clone()),
                                rhs,
                                values_defined_here,
                            )
                            .await;
                        }

                        if let Some(val) = val {
                            if !self
                                .set_values_in_context(holder, &val, &rhs, values_defined_here)
                                .await
                            {
                                return false;
                            }
                        }
                    } else {
                        return false;
                    }
                }

                true
            }
            (Syntax::MapMatch(lhs), Syntax::Context(ctx_id, _system_id, _)) => {
                let mut ctx = holder.get(*ctx_id).await.unwrap();

                for (key, key_into, val, is_id) in lhs.iter() {
                    debug!("{}", ctx_id);
                    if let Some(rhs) = ctx.get_global(key).await {
                        if *is_id {
                            self.set_values_in_context(
                                holder,
                                &Syntax::Id(key.clone()),
                                &rhs,
                                values_defined_here,
                            )
                            .await;
                        } else if let Some(key) = key_into {
                            self.set_values_in_context(
                                holder,
                                &Syntax::Id(key.clone()),
                                &rhs,
                                values_defined_here,
                            )
                            .await;
                        }

                        if let Some(val) = val {
                            if !self
                                .set_values_in_context(holder, &val, &rhs, values_defined_here)
                                .await
                            {
                                return false;
                            }
                        }
                    } else {
                        return false;
                    }
                }

                true
            }
            (Syntax::ExplicitExpr(lhs), rhs) => {
                self.set_values_in_context(holder, lhs, rhs, values_defined_here)
                    .await
            }
            (lhs, Syntax::ExplicitExpr(rhs)) => {
                self.set_values_in_context(holder, lhs, rhs, values_defined_here)
                    .await
            }
            (expr0, expr1) => expr0.eval_equal(expr1).await,
        }
    }
}

async fn build_fn(name: &String, fns: &[(Vec<Syntax>, Syntax)]) -> Syntax {
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

    build_fn_with_args(&params, &params, &fns).reduce().await
}

impl PartialEq for PrivateContext {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Into<Context> for PrivateContext {
    fn into(self) -> Context {
        Context {
            id: self.id,
            ctx: Arc::new(Mutex::new(self)),
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

    pub fn handler(&self, holder: ContextHolder) -> ContextHandler {
        ContextHandler {
            id: self.get_id(),
            holder,
        }
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
        self.id_to_ctx.get(&id).map(|ctx| ctx.clone())
    }

    pub fn create_context(&mut self, name: String, path: Option<PathBuf>) -> Context {
        let id = self.last_id;
        let ctx: Context = PrivateContext::new(id, name, path).into();
        self.last_id += 1;

        self.id_to_ctx.insert(id, ctx.clone());

        ctx
    }

    pub fn set_path(&mut self, name: &String, ctx: &Context) {
        self.path_to_ctx.insert(name.clone(), ctx.clone());
    }

    pub fn get_path(&mut self, name: &String) -> Option<Context> {
        self.path_to_ctx.get(name).map(|ctx| ctx.clone())
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

    pub async fn set_values_in_context(
        &mut self,
        holder: &mut ContextHolder,
        lhs: &Syntax,
        rhs: &Syntax,
        values_defined_here: &mut Vec<String>,
    ) -> bool {
        self.holder
            .get(self.id)
            .await
            .unwrap()
            .set_values_in_context(holder, lhs, rhs, values_defined_here)
            .await
    }

    pub fn get_holder(&self) -> ContextHolder {
        self.holder.clone()
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
        if let Some(_) = self.handler.lock().await.get(id) {
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

    pub async fn set_path(&mut self, name: &String, ctx: &Context) {
        self.handler.lock().await.set_path(name, ctx);
    }

    pub async fn get_path(&mut self, name: &String) -> Option<Context> {
        self.handler.lock().await.get_path(name)
    }
}
