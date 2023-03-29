///! This module is for creating an untyped AST and creating an typed AST from it
use std::{iter::Peekable, str::FromStr};

use bigdecimal::BigDecimal;
use num::Num;
use rowan::{GreenNodeBuilder, NodeOrToken};

use crate::{errors::InterpreterError, execute::Syntax};

/// SyntaxKinds for the untyped syntax tree created with *rowan*
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    Lambda,
    ParenLeft,
    ParenRight,
    LstLeft,
    LstRight,
    MapLeft,
    MapRight,
    OpAdd,
    OpSub,
    OpMul,
    OpDiv,
    Unpack,
    OpPeriod,
    OpComma,
    OpAsg,
    OpEq,
    OpStrictEq,
    OpNeq,
    OpGeq,
    OpLeq,
    OpGt,
    OpLt,
    OpStrictNeq,
    OpMap,
    OpPipe,
    NewLine,
    Pipe,

    Semicolon,

    Call,
    Flt,
    Int,

    KwLet,
    KwIn,
    KwExport,
    KwImport,
    If,
    IfLet,
    KwThen,
    KwElse,
    Any,
    Id,
    Atom,
    Str,
    Lst,
    LstMatch,
    Map,
    MapMatch,
    MapElement,
    ExplicitExpr,

    Error,

    BiOp,
    Root,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {}
impl rowan::Language for Lang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= SyntaxKind::Root as u16);
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<Lang>;
pub type SyntaxToken = rowan::SyntaxToken<Lang>;
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

/// The parser responsible for parsing the tokens into an untyped syntax tree
pub struct Parser<I: Iterator<Item = (SyntaxKind, String)>> {
    builder: GreenNodeBuilder<'static>,
    errors: Vec<String>,
    iter: Peekable<I>,
    tuple: Vec<bool>,
}

impl<I: Iterator<Item = (SyntaxKind, String)>> Parser<I> {
    pub fn new(builder: GreenNodeBuilder<'static>, iter: Peekable<I>) -> Self {
        Self {
            builder,
            iter,
            errors: Default::default(),
            tuple: Vec::new(),
        }
    }
}

impl<I: Iterator<Item = (SyntaxKind, String)>> Parser<I> {
    fn is_tuple(&mut self) -> bool {
        if let Some(result) = self.tuple.last() {
            *result
        } else {
            false
        }
    }

    fn peek(&mut self) -> Option<SyntaxKind> {
        self.iter.peek().map(|&(t, _)| t)
    }

    /// Bump the current token to rowan
    fn bump(&mut self) {
        if let Some((token, string)) = self.iter.next() {
            self.builder.token(token.into(), string.as_str());
        }
    }

    /// Parse value or statement
    fn parse_val(&mut self, first: bool, allow_empty: bool) -> bool {
        match self.peek() {
            Some(SyntaxKind::Int) => {
                self.bump();
                true
            }
            Some(SyntaxKind::Flt) => {
                self.bump();
                true
            }
            Some(SyntaxKind::Str) => {
                self.bump();
                true
            }
            Some(SyntaxKind::Any) => {
                self.bump();
                true
            }
            Some(SyntaxKind::Id) => {
                self.bump();
                true
            }
            Some(SyntaxKind::Atom) => {
                self.bump();
                true
            }
            Some(SyntaxKind::KwExport) => {
                self.iter.next();

                if self.peek() == Some(SyntaxKind::Id) {
                    self.builder.start_node(SyntaxKind::KwExport.into());
                    self.bump();
                    self.builder.finish_node();

                    true
                } else {
                    self.errors
                        .push(format!("Expected identifier after export keyword"));

                    self.builder.start_node(SyntaxKind::Error.into());
                    self.bump();
                    self.builder.finish_node();

                    false
                }
            }
            Some(SyntaxKind::KwImport) => {
                self.iter.next();

                if self.peek() == Some(SyntaxKind::Str) {
                    self.builder.start_node(SyntaxKind::KwImport.into());
                    self.bump();
                    self.builder.finish_node();

                    true
                } else {
                    self.errors
                        .push(format!("Expected string after import keyword"));

                    self.builder.start_node(SyntaxKind::Error.into());
                    self.bump();
                    self.builder.finish_node();

                    false
                }
            }
            Some(SyntaxKind::OpPipe) if first => {
                self.iter.next(); // skip |

                let checkpoint = self.builder.checkpoint();
                if self.parse_expr(first) {
                    self.builder
                        .start_node_at(checkpoint, SyntaxKind::Pipe.into());
                    self.builder.finish_node();

                    true
                } else {
                    false
                }
            }
            Some(SyntaxKind::LstLeft) => {
                self.tuple.push(false);

                self.iter.next(); // skip [
                let checkpoint = self.builder.checkpoint();

                if Some(SyntaxKind::LstRight) != self.peek() {
                    self.parse_expr(false);
                    if Some(SyntaxKind::OpComma) == self.peek() {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::Lst.into());
                        while Some(SyntaxKind::OpComma) == self.peek() {
                            self.iter.next();
                            self.parse_expr(false);
                        }
                    } else if Some(SyntaxKind::Semicolon) == self.peek()
                        || Some(SyntaxKind::OpMap) == self.peek()
                    {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::LstMatch.into());
                        while Some(SyntaxKind::Semicolon) == self.peek()
                            || Some(SyntaxKind::OpMap) == self.peek()
                        {
                            self.iter.next();
                            self.parse_expr(false);
                        }
                    } else {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::Lst.into());
                    }

                    if Some(SyntaxKind::LstRight) != self.peek() {
                        self.errors.push(format!("Expected ]"));
                    }
                } else {
                    self.builder
                        .start_node_at(checkpoint, SyntaxKind::Lst.into());
                }

                if Some(SyntaxKind::LstRight) == self.peek() {
                    self.iter.next(); // skip maybe ]
                }

                self.builder.finish_node();
                self.tuple.pop().unwrap();

                true
            }
            Some(SyntaxKind::MapLeft) => {
                self.iter.next();

                let checkpoint = self.builder.checkpoint();

                if Some(SyntaxKind::MapRight) != self.peek() {
                    self.tuple.push(false);

                    let has_errors = false;
                    let mut is_match = false;

                    while [SyntaxKind::Id, SyntaxKind::Str]
                        .iter()
                        .any(|x| Some(*x) == self.peek())
                    {
                        self.builder.start_node(SyntaxKind::MapElement.into());
                        if Some(SyntaxKind::Str) == self.peek() {
                            self.bump();

                            if Some(SyntaxKind::Id) == self.peek() {
                                self.bump();
                                is_match = true;
                            }
                        } else {
                            self.bump();
                        }

                        if Some(SyntaxKind::OpMap) == self.peek() {
                            self.iter.next(); // skip :

                            self.parse_expr(false);
                        } else {
                            is_match = true;
                        }

                        self.builder.finish_node();

                        if Some(SyntaxKind::OpComma) == self.peek() {
                            self.iter.next();
                        }
                    }

                    if has_errors {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::Error.into());
                    } else if is_match {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::MapMatch.into());
                    } else {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::Map.into());
                    }

                    self.builder.finish_node();

                    self.tuple.pop().unwrap();
                }

                if Some(SyntaxKind::MapRight) == self.peek() {
                    self.iter.next(); // skip
                }

                true
            }
            Some(SyntaxKind::ParenLeft) => {
                self.tuple.push(true);

                self.iter.next(); // skip

                self.builder.start_node(SyntaxKind::ExplicitExpr.into());
                self.parse_expr(false);
                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::ParenRight)
                    .unwrap_or(false)
                {
                    self.iter.next(); // skip
                } else {
                    self.errors.push(format!("Expected )"));
                }

                self.builder.finish_node();

                self.tuple.pop().unwrap();

                true
            }
            Some(SyntaxKind::If) => {
                self.iter.next(); // skip
                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::KwLet)
                    .unwrap_or(false)
                {
                    // parse an if-let statement
                    self.iter.next(); // skip
                    self.builder.start_node(SyntaxKind::IfLet.into());
                    self.parse_let_args(SyntaxKind::KwThen);
                } else {
                    // parse a normal if condition
                    self.builder.start_node(SyntaxKind::If.into());
                    self.parse_expr(false);

                    if self
                        .peek()
                        .map(|tok| tok == SyntaxKind::KwThen)
                        .unwrap_or(false)
                    {
                        self.iter.next(); // skip
                    } else {
                        self.errors.push(format!("Expected 'then'"));
                    }
                }

                // parse the true case
                self.parse_expr(false);

                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::KwElse)
                    .unwrap_or(false)
                {
                    self.iter.next(); // skip
                } else {
                    self.errors.push(format!("Expected 'else'"));
                }

                // parse the else case
                self.parse_expr(false);

                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::KwLet) => {
                self.iter.next(); // Skip let

                self.builder.start_node(SyntaxKind::KwLet.into());
                self.parse_let_args(SyntaxKind::KwIn);
                self.parse_expr(false);
                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::Lambda) => {
                self.iter.next(); // Skip \
                self.builder.start_node(SyntaxKind::Lambda.into());

                if !self
                    .peek()
                    .map(|tok| tok == SyntaxKind::Id)
                    .unwrap_or(false)
                {
                    self.errors
                        .push(format!("Expected identifier after lambda"));
                }

                self.bump(); // parse Id

                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::OpAsg)
                    .unwrap_or(false)
                {
                    self.iter.next(); // ignore
                }

                // parse the lambda expression
                self.parse_expr(false);
                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::OpSub) => {
                if allow_empty {
                    false
                } else {
                    self.iter.next(); // Skip -

                    self.builder.start_node(SyntaxKind::BiOp.into());
                    self.builder.token(SyntaxKind::Int.into(), "0");
                    self.builder.token(SyntaxKind::OpSub.into(), "-");

                    let result = self.parse_expr(false);

                    self.builder.finish_node();

                    result
                }
            }
            _ => {
                if !allow_empty {
                    self.errors.push(format!("Expected primary expression"));

                    self.builder.start_node(SyntaxKind::Error.into());
                    self.bump();
                    self.builder.finish_node();
                }

                false
            }
        }
    }

    /// Parses the let assignments in an let or if-let statement
    ///
    /// ## Arguments
    ///
    /// - end: the terminating symbol of the assignments
    fn parse_let_args(&mut self, end: SyntaxKind) {
        loop {
            self.builder.start_node(SyntaxKind::BiOp.into());
            self.parse_call(false);

            if self
                .peek()
                .map(|tok| tok == SyntaxKind::OpAsg)
                .unwrap_or(false)
            {
                self.bump();
            } else {
                self.errors.push(format!("Expected = in let expression"));
            }

            self.parse_expr(false);

            self.builder.finish_node();

            if self
                .peek()
                .map(|tok| tok == SyntaxKind::Semicolon)
                .unwrap_or(false)
            {
                self.iter.next();
                continue;
            }

            if self.peek().map(|tok| tok == end).unwrap_or(false) {
                self.iter.next();
                break;
            }

            match end {
                SyntaxKind::KwIn => self.errors.push(format!("Expected either ';' or 'in'")),
                _ => self.errors.push(format!("Expected either ';' or 'then'")),
            }
            break;
        }
    }

    /// Handle a binary operator expression (or just next)
    fn handle_operation(
        &mut self,
        first: bool,
        tokens: &[SyntaxKind],
        next: fn(&mut Self, bool) -> bool,
    ) -> bool {
        let checkpoint = self.builder.checkpoint();
        if !next(self, first) {
            return false;
        }

        while self.peek().map(|t| tokens.contains(&t)).unwrap_or(false) {
            self.builder
                .start_node_at(checkpoint, SyntaxKind::BiOp.into());
            self.bump();
            next(self, false);
            self.builder.finish_node();
        }

        true
    }

    fn parse_period(&mut self, first: bool, next: fn(&mut Self, bool) -> bool) -> bool {
        self.handle_operation(first, &[SyntaxKind::OpPeriod], next)
    }

    /// Parse a lambda/function call expression
    fn parse_call(&mut self, first: bool) -> bool {
        let maybe_call = self.builder.checkpoint();
        if !self.parse_period(first, |this, first| this.parse_val(first, false)) {
            return false;
        }

        while self.parse_period(false, |this, first| this.parse_val(first, true)) {
            self.builder
                .start_node_at(maybe_call, SyntaxKind::Call.into());
            self.builder.finish_node();
        }

        true
    }

    fn parse_cmp(&mut self, first: bool) -> bool {
        self.handle_operation(
            first,
            &[
                SyntaxKind::OpEq,
                SyntaxKind::OpNeq,
                SyntaxKind::OpStrictEq,
                SyntaxKind::OpStrictNeq,
                SyntaxKind::OpGeq,
                SyntaxKind::OpLeq,
                SyntaxKind::OpGt,
                SyntaxKind::OpLt,
            ],
            Self::parse_call,
        )
    }

    fn parse_mul(&mut self, first: bool) -> bool {
        self.handle_operation(
            first,
            &[SyntaxKind::OpMul, SyntaxKind::OpDiv],
            Self::parse_cmp,
        )
    }

    fn parse_add(&mut self, first: bool) -> bool {
        self.handle_operation(
            first,
            &[SyntaxKind::OpAdd, SyntaxKind::OpSub],
            Self::parse_mul,
        )
    }

    fn parse_pipe(&mut self, first: bool) -> bool {
        self.handle_operation(first, &[SyntaxKind::OpPipe], Self::parse_add)
    }

    fn parse_tuple(&mut self, first: bool) -> bool {
        if self.is_tuple() {
            self.handle_operation(first, &[SyntaxKind::OpComma], Self::parse_pipe)
        } else {
            self.parse_pipe(first)
        }
    }

    fn parse_asg(&mut self, first: bool) -> bool {
        self.handle_operation(first, &[SyntaxKind::OpAsg], Self::parse_tuple)
    }

    fn parse_expr(&mut self, first: bool) -> bool {
        self.parse_asg(first)
    }

    pub fn parse(mut self) -> (Box<SyntaxElement>, Vec<String>) {
        self.builder.start_node(SyntaxKind::Root.into());
        self.parse_expr(true);
        while self.peek() == Some(SyntaxKind::NewLine) {
            self.iter.next();

            let peeked = self.peek();
            if !peeked.is_none() && peeked != Some(SyntaxKind::NewLine) {
                self.parse_expr(true);
            }
        }

        self.builder.finish_node();

        (
            Box::new(SyntaxNode::new_root(self.builder.finish()).into()),
            self.errors,
        )
    }
}

impl TryInto<Syntax> for SyntaxElement {
    type Error = InterpreterError;

    fn try_into(self) -> Result<Syntax, Self::Error> {
        let kind: SyntaxKind = self.kind().into();
        match self {
            NodeOrToken::Node(node) => {
                let mut children = node.children_with_tokens();

                let expected_expr = || InterpreterError::ExpectedExpression();

                match kind {
                    SyntaxKind::Root => {
                        let children: Vec<SyntaxElement> = children.collect();
                        if children.len() == 0 {
                            Err(InterpreterError::InternalError(format!(
                                "Root must have at least one child"
                            )))
                        } else if children.len() == 1 {
                            children.into_iter().next().unwrap().try_into()
                        } else {
                            Ok(Syntax::Program(
                                children
                                    .into_iter()
                                    .map(|child| child.try_into())
                                    .try_collect()?,
                            ))
                        }
                    }
                    SyntaxKind::ExplicitExpr => {
                        let child = children
                            .next()
                            .ok_or(InterpreterError::ExpectedExpression())?;

                        Ok(Syntax::ExplicitExpr(Box::new(child.try_into()?)))
                    }
                    SyntaxKind::Lambda => {
                        let id = children
                            .next()
                            .map(|x| {
                                if let NodeOrToken::Token(tok) = x {
                                    Some(tok)
                                } else {
                                    None
                                }
                            })
                            .flatten()
                            .ok_or(InterpreterError::ExpectedIdentifier())?;

                        let expr = children.next().ok_or(expected_expr())?;

                        Ok(Syntax::Lambda(
                            id.text().to_string(),
                            Box::new(expr.try_into()?),
                        ))
                    }
                    SyntaxKind::Call => {
                        let lhs = children.next().ok_or(expected_expr())?;
                        let rhs = children.next().ok_or(expected_expr())?;

                        Ok(Syntax::Call(
                            Box::new(lhs.try_into()?),
                            Box::new(rhs.try_into()?),
                        ))
                    }
                    SyntaxKind::KwLet => {
                        let children: Vec<SyntaxElement> = children.collect();
                        let asgs: Vec<(SyntaxElement, SyntaxElement)> = children.clone()
                            [..children.len() - 1]
                            .iter()
                            .map(|child| {
                                if let NodeOrToken::Node(node) = child {
                                    let mut children = node.children_with_tokens();
                                    let lhs = children.next().unwrap();
                                    children.next();
                                    let rhs = children.next().unwrap();

                                    (lhs, rhs)
                                } else {
                                    panic!("Not allowed here!");
                                }
                            })
                            .collect();

                        fn build_let(
                            asgs: &[(SyntaxElement, SyntaxElement)],
                            expr: SyntaxElement,
                        ) -> Result<Syntax, InterpreterError> {
                            if asgs.len() == 0 {
                                expr.try_into()
                            } else {
                                Ok(Syntax::Let(
                                    (
                                        Box::new(asgs[0].0.clone().try_into()?),
                                        Box::new(asgs[0].1.clone().try_into()?),
                                    ),
                                    Box::new(build_let(&asgs[1..], expr)?),
                                ))
                            }
                        }

                        let expr: SyntaxElement = children.last().unwrap().clone();
                        build_let(&asgs, expr)
                    }
                    SyntaxKind::BiOp => {
                        let lhs = children.next().ok_or(expected_expr())?;
                        let op = children
                            .next()
                            .map(|x| {
                                if let NodeOrToken::Token(tok) = x {
                                    Some(tok)
                                } else {
                                    None
                                }
                            })
                            .flatten()
                            .ok_or(InterpreterError::ExpectedOperator())?;
                        let rhs = children.next().ok_or(expected_expr())?;

                        if op.kind() == SyntaxKind::OpAsg {
                            Ok(Syntax::Asg(
                                Box::new(lhs.try_into()?),
                                Box::new(rhs.try_into()?),
                            ))
                        } else if op.kind() == SyntaxKind::OpComma {
                            Ok(Syntax::Tuple(
                                Box::new(lhs.try_into()?),
                                Box::new(rhs.try_into()?),
                            ))
                        } else {
                            use crate::execute::BiOpType::*;

                            Ok(Syntax::BiOp(
                                (match op.kind() {
                                    SyntaxKind::OpEq => Ok(OpEq),
                                    SyntaxKind::OpNeq => Ok(OpNeq),
                                    SyntaxKind::OpStrictEq => Ok(OpStrictEq),
                                    SyntaxKind::OpStrictNeq => Ok(OpStrictNeq),
                                    SyntaxKind::OpAdd => Ok(OpAdd),
                                    SyntaxKind::OpSub => Ok(OpSub),
                                    SyntaxKind::OpMul => Ok(OpMul),
                                    SyntaxKind::OpDiv => Ok(OpDiv),
                                    SyntaxKind::OpGeq => Ok(OpGeq),
                                    SyntaxKind::OpLeq => Ok(OpLeq),
                                    SyntaxKind::OpGt => Ok(OpGt),
                                    SyntaxKind::OpLt => Ok(OpLt),
                                    SyntaxKind::OpPipe => Ok(OpPipe),
                                    SyntaxKind::OpPeriod => Ok(OpPeriod),
                                    _ => Err(InterpreterError::InvalidOperator()),
                                })?,
                                Box::new(lhs.try_into()?),
                                Box::new(rhs.try_into()?),
                            ))
                        }
                    }
                    SyntaxKind::IfLet => {
                        let children: Vec<SyntaxElement> = children.collect();
                        let asgs: Vec<(Syntax, Syntax)> = children.clone()[..children.len() - 2]
                            .iter()
                            .map(|child| -> Result<(Syntax, Syntax), InterpreterError> {
                                if let NodeOrToken::Node(node) = child {
                                    let mut children = node.children_with_tokens();
                                    let lhs = children
                                        .next()
                                        .ok_or(InterpreterError::ExpectedLHSExpression())?;
                                    children.next();
                                    let rhs = children
                                        .next()
                                        .ok_or(InterpreterError::ExpectedRHSExpression())?;

                                    Ok((lhs.try_into()?, rhs.try_into()?))
                                } else {
                                    panic!("Not allowed here!");
                                }
                            })
                            .fold(
                                Ok(Vec::<(Syntax, Syntax)>::new()),
                                |lhs, rhs| -> Result<Vec<(Syntax, Syntax)>, InterpreterError> {
                                    if let Ok(mut lhs) = lhs {
                                        lhs.push(rhs?);
                                        Ok(lhs)
                                    } else {
                                        lhs
                                    }
                                },
                            )?;

                        let expr_true = children[children.len() - 2].clone();
                        let expr_false = children[children.len() - 1].clone();

                        Ok(Syntax::IfLet(
                            asgs,
                            Box::new(expr_true.try_into()?),
                            Box::new(expr_false.try_into()?),
                        ))
                    }
                    SyntaxKind::If => {
                        let cond = children.next().ok_or(expected_expr())?;
                        let true_expr = children.next().ok_or(expected_expr())?;
                        let false_expr = children.next().ok_or(expected_expr())?;

                        Ok(Syntax::If(
                            Box::new(cond.try_into()?),
                            Box::new(true_expr.try_into()?),
                            Box::new(false_expr.try_into()?),
                        ))
                    }
                    SyntaxKind::Lst => {
                        let mut lst = Vec::new();
                        for child in children {
                            lst.push(child.try_into()?);
                        }

                        Ok(Syntax::Lst(lst))
                    }
                    SyntaxKind::LstMatch => {
                        let mut lst = Vec::new();
                        for child in children {
                            lst.push(child.try_into()?);
                        }

                        Ok(Syntax::LstMatch(lst))
                    }
                    SyntaxKind::Map => {
                        let mut map = Vec::new();
                        let mut children = children.into_iter();
                        while let Some(NodeOrToken::Node(map_element)) = children.next() {
                            let mut children = map_element.children_with_tokens().into_iter();
                            let id = children.next().unwrap().into_token().unwrap();
                            let val: SyntaxElement = children.next().unwrap().into();

                            map.push((
                                id.text().to_string(),
                                (val.try_into()?, {
                                    let id: SyntaxKind = id.kind().into();
                                    id == SyntaxKind::Id
                                }),
                            ));
                        }

                        Ok(Syntax::Map(map.into_iter().collect()))
                    }
                    SyntaxKind::MapMatch => {
                        let mut map = Vec::new();
                        let mut children = children.into_iter();
                        while let Some(NodeOrToken::Node(map_element)) = children.next() {
                            let mut children = map_element.children_with_tokens().peekable();
                            let key = children.next().unwrap().into_token().unwrap();
                            let key_into = if key.kind() == SyntaxKind::Id {
                                None
                            } else if children
                                .peek()
                                .map(|x| x.kind() == SyntaxKind::Id)
                                .unwrap_or(false)
                            {
                                Some(
                                    children
                                        .next()
                                        .unwrap()
                                        .into_token()
                                        .unwrap()
                                        .text()
                                        .to_string(),
                                )
                            } else {
                                None
                            };

                            let val = children
                                .next()
                                .map(|x| -> SyntaxElement { x.into() })
                                .map(|x| x.try_into())
                                .transpose()?;
                            map.push((
                                key.text().to_string(),
                                key_into,
                                val,
                                key.kind() == SyntaxKind::Id,
                            ));
                        }

                        Ok(Syntax::MapMatch(map))
                    }
                    SyntaxKind::Pipe => {
                        let child = children
                            .next()
                            .ok_or(InterpreterError::ExpectedExpression())?;

                        Ok(Syntax::Pipe(Box::new(child.try_into()?)))
                    }
                    SyntaxKind::KwExport => {
                        let child = children
                            .next()
                            .ok_or(InterpreterError::ExpectedIdentifier())?
                            .into_token()
                            .ok_or(InterpreterError::ExpectedIdentifier())?;

                        Ok(Syntax::Export(child.text().to_string()))
                    }
                    SyntaxKind::KwImport => {
                        let child = children
                            .next()
                            .ok_or(InterpreterError::ExpectedIdentifier())?
                            .into_token()
                            .ok_or(InterpreterError::ExpectedIdentifier())?;

                        Ok(Syntax::Import(child.text().to_string()))
                    }
                    _ => Err(InterpreterError::UnexpectedExpressionEmpty(kind)),
                }
            }
            NodeOrToken::Token(tok) => match kind {
                SyntaxKind::Id => Ok(Syntax::Id(tok.text().to_string())),
                SyntaxKind::Int => Ok(Syntax::ValInt(
                    num::BigInt::from_str_radix(tok.text(), 10)
                        .map_err(|_| InterpreterError::ExpectedInteger())?,
                )),
                SyntaxKind::Flt => Ok(Syntax::ValFlt(
                    tok.text()
                        .parse()
                        .map_err(|_| InterpreterError::ExpectedFloat())?,
                )),
                SyntaxKind::Str => Ok(Syntax::ValStr(tok.text().to_string())),
                SyntaxKind::Atom => Ok(Syntax::ValAtom(tok.text().to_string())),
                SyntaxKind::Any => Ok(Syntax::ValAny()),
                _ => Err(InterpreterError::UnexpectedTokenEmpty()),
            },
        }
    }
}

pub fn print_ast(indent: usize, element: &SyntaxElement) {
    let kind: SyntaxKind = element.kind().into();
    print!("{:indent$}", "", indent = indent);
    match element {
        NodeOrToken::Node(node) => {
            println!("- {:?}", kind);
            for child in node.children_with_tokens() {
                print_ast(indent + 2, &child);
            }
        }

        NodeOrToken::Token(token) => println!("- {:?} {:?}", token.text(), kind),
    }
}
