///! This module is for creating an untyped AST and creating an typed AST from it
use std::collections::VecDeque;

use log::warn;
use num::Num;
use regex::Regex;
use rowan::{GreenNodeBuilder, NodeOrToken};

use crate::{
    errors::InterpreterError,
    execute::{Syntax, UnOpType},
};

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
    OpPow,
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
    OpImmediate,
    OpMatchCase,
    NewLine,
    Pipe,

    Semicolon,

    Call,
    Flt,
    Int,

    KwLet,
    KwIn,
    If,
    IfLet,
    KwThen,
    KwElse,
    KwMatch,
    Any,
    Id,
    Atom,
    Str,
    Rg,
    Lst,
    LstMatch,
    Map,
    MapMatch,
    MapElement,
    ExplicitExpr,

    Error,

    FnOp,
    BiOp,
    UnOp,
    Root,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

impl SyntaxKind {
    fn is_bi_op(&self) -> bool {
        match self {
            Self::OpPow
            | Self::OpAdd
            | Self::OpSub
            | Self::OpMul
            | Self::OpDiv
            | Self::OpPeriod
            | Self::OpComma
            | Self::OpAsg
            | Self::OpEq
            | Self::OpStrictEq
            | Self::OpNeq
            | Self::OpGeq
            | Self::OpLeq
            | Self::OpGt
            | Self::OpLt
            | Self::OpStrictNeq => true,
            _ => false,
        }
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
    iter: I,
    buffer: VecDeque<(SyntaxKind, String)>,
    tuple: Vec<bool>,
    line: usize,
    is_let: Vec<bool>,
}

impl<I: Iterator<Item = (SyntaxKind, String)>> Parser<I> {
    pub fn new(builder: GreenNodeBuilder<'static>, iter: I) -> Self {
        Self {
            builder,
            iter,
            buffer: VecDeque::new(),
            errors: Default::default(),
            tuple: Vec::new(),
            line: 1,
            is_let: Vec::new(),
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

    fn bufferize(&mut self, len: usize) -> bool {
        for _ in 0..len {
            if let Some(tok) = self.iter.next() {
                self.buffer.push_back(tok);
            } else {
                return false;
            }
        }

        return true;
    }

    fn peek(&mut self) -> Option<SyntaxKind> {
        self.peek_nth(0)
    }

    fn peek_nth(&mut self, offset: usize) -> Option<SyntaxKind> {
        if self.buffer.len() <= offset {
            if !self.bufferize(offset - self.buffer.len() + 1) {
                return None;
            }
        }

        Some(self.buffer.get(offset)?.0)
    }

    fn next(&mut self) -> Option<(SyntaxKind, String)> {
        if let Some((token, string)) = self.buffer.pop_front() {
            Some((token, string))
        } else {
            self.iter.next()
        }
    }

    /// Bump the current token to rowan
    fn bump(&mut self) {
        if let Some((token, string)) = self.next() {
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
            Some(SyntaxKind::Rg) => {
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
            Some(SyntaxKind::OpPipe) if first => {
                self.next(); // skip |

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
            Some(op @ SyntaxKind::OpImmediate) => {
                self.builder.start_node(SyntaxKind::UnOp.into());
                self.bump(); // eat operator

                let result = self.parse_expr(false);
                self.builder.finish_node();

                result
            }
            Some(SyntaxKind::LstLeft) => {
                self.tuple.push(false);

                self.next(); // skip [
                let checkpoint = self.builder.checkpoint();

                if Some(SyntaxKind::LstRight) != self.peek() {
                    self.parse_expr(false);
                    if Some(SyntaxKind::OpComma) == self.peek() {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::Lst.into());
                        while Some(SyntaxKind::OpComma) == self.peek() {
                            self.next();
                            self.parse_expr(false);
                        }
                    } else if Some(SyntaxKind::OpMap) == self.peek() {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::LstMatch.into());
                        while Some(SyntaxKind::OpMap) == self.peek() {
                            self.next();
                            self.parse_expr(false);
                        }
                    } else {
                        self.builder
                            .start_node_at(checkpoint, SyntaxKind::Lst.into());
                    }

                    if Some(SyntaxKind::LstRight) != self.peek() {
                        self.errors.push("Expected ]".to_string());
                    }
                } else {
                    self.builder
                        .start_node_at(checkpoint, SyntaxKind::Lst.into());
                }

                if Some(SyntaxKind::LstRight) == self.peek() {
                    self.next(); // skip maybe ]
                }

                self.builder.finish_node();
                self.tuple.pop().unwrap();

                true
            }
            Some(SyntaxKind::MapLeft) => {
                self.next();
                self.skip_newlines();

                let checkpoint = self.builder.checkpoint();

                if self.peek() != Some(SyntaxKind::MapRight) {
                    self.tuple.push(false);

                    let has_errors = false;
                    let mut is_match = false;

                    while {
                        let peeked = self.peek();
                        [SyntaxKind::Id, SyntaxKind::Str]
                            .iter()
                            .any(|x| Some(*x) == peeked)
                    } {
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
                            self.next(); // skip :

                            self.parse_expr(false);
                        } else {
                            is_match = true;
                        }

                        self.builder.finish_node();

                        if Some(SyntaxKind::OpComma) == self.peek() {
                            self.next();
                            self.skip_newlines();
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
                } else {
                    self.builder.start_node(SyntaxKind::Map.into());
                    self.builder.finish_node();
                }

                self.skip_newlines();

                if Some(SyntaxKind::MapRight) == self.peek() {
                    self.next(); // skip
                }

                true
            }
            Some(SyntaxKind::ParenLeft) => {
                self.next(); // skip
                self.tuple.push(true);
                self.skip_newlines();

                // Special case for operators-as-functions
                if self.peek_nth(0).map(|tok| tok.is_bi_op()).unwrap_or(false)
                    && self.peek_nth(1) == Some(SyntaxKind::ParenRight)
                {
                    self.builder.start_node(SyntaxKind::FnOp.into());
                    self.bump();
                    self.builder.finish_node();

                    self.next(); // skip
                    self.tuple.pop().unwrap();

                    return true;
                }

                self.builder.start_node(SyntaxKind::ExplicitExpr.into());
                let checkpoint = self.builder.checkpoint();
                let mut expr_cnt = 0;
                loop {
                    if self.peek() == Some(SyntaxKind::ParenRight) {
                        break;
                    }

                    self.parse_expr(false);

                    expr_cnt += 1;
                    if self.peek() == Some(SyntaxKind::Semicolon) {
                        self.next();
                        self.skip_newlines();
                    } else {
                        break;
                    }
                }

                if expr_cnt > 1 {
                    self.builder
                        .start_node_at(checkpoint, SyntaxKind::Root.into());
                    self.builder.finish_node();
                }

                self.skip_newlines();

                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::ParenRight)
                    .unwrap_or(false)
                {
                    self.next(); // skip
                } else {
                    self.errors.push("Expected )".to_string());
                }

                self.builder.finish_node();

                self.tuple.pop().unwrap();

                true
            }
            Some(SyntaxKind::If) => {
                self.next(); // skip
                self.skip_newlines();

                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::KwLet)
                    .unwrap_or(false)
                {
                    // parse an if-let statement
                    self.next(); // skip
                    self.skip_newlines();
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
                        self.next(); // skip
                        self.skip_newlines();
                    } else {
                        self.errors.push("Expected 'then'".to_string());
                    }
                }

                // parse the true case
                self.parse_expr(false);

                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::KwElse)
                    .unwrap_or(false)
                {
                    self.next(); // skip
                    self.skip_newlines();
                } else {
                    self.errors.push("Expected 'else'".to_string());
                }

                // parse the else case
                self.parse_expr(false);

                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::KwLet) => {
                self.next(); // Skip let
                self.skip_newlines();

                self.builder.start_node(SyntaxKind::KwLet.into());
                self.parse_let_args(SyntaxKind::KwIn);
                self.skip_newlines();

                self.parse_expr(false);
                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::KwMatch) => {
                self.next(); // skip match
                self.skip_newlines();

                self.builder.start_node(SyntaxKind::KwMatch.into());
                self.parse_expr(false);
                self.skip_newlines();

                if self.peek() == Some(SyntaxKind::KwThen) {
                    self.next();
                } else {
                    self.errors
                        .push("Expected 'then' after match expression".to_string());
                }

                self.skip_newlines();

                loop {
                    if !self.parse_expr(false) {
                        break;
                    }

                    self.skip_newlines();

                    if self.peek() == Some(SyntaxKind::OpMatchCase) {
                        self.next();
                        self.skip_newlines();
                        self.parse_expr(false);
                        self.skip_newlines();
                    } else {
                        self.errors.push("Expected '=>' case operator".to_string());
                    }

                    if self.peek() == Some(SyntaxKind::OpComma) {
                        self.next();
                        self.skip_newlines();
                    } else {
                        break;
                    }
                }

                if self.peek() == Some(SyntaxKind::Semicolon) {
                    self.next();
                } else {
                    self.errors.push("Expected semicolon ';'".to_string());
                }

                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::Lambda) => {
                self.next(); // Skip \
                self.builder.start_node(SyntaxKind::Lambda.into());

                if !self
                    .peek()
                    .map(|tok| tok == SyntaxKind::Id)
                    .unwrap_or(false)
                {
                    self.errors
                        .push("Expected identifier after lambda".to_string());
                }

                self.bump(); // parse Id

                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::OpAsg)
                    .unwrap_or(false)
                {
                    self.next(); // ignore
                }

                self.skip_newlines();

                // parse the lambda expression
                self.parse_expr(false);
                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::OpSub) => {
                if allow_empty {
                    false
                } else {
                    self.next(); // Skip -

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
                    self.errors.push("Expected primary expression".to_string());

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
        self.is_let.push(true);
        let mut count = 0;
        loop {
            // Allow the 'then' or 'in' the next line
            if count > 0 && self.peek().map(|tok| tok == end).unwrap_or(false) {
                self.next();
                self.skip_newlines();
                break;
            }

            self.builder.start_node(SyntaxKind::BiOp.into());
            self.parse_call(false);

            if self
                .peek()
                .map(|tok| tok == SyntaxKind::OpAsg)
                .unwrap_or(false)
            {
                self.bump();
            } else {
                self.errors.push("Expected = in let expression".to_string());
            }

            self.parse_expr(false);

            self.builder.finish_node();

            self.skip_newlines();

            if self
                .peek()
                .map(|tok| tok == SyntaxKind::Semicolon)
                .unwrap_or(false)
            {
                self.next();
                self.skip_newlines();
                count += 1;
                continue;
            }

            if self.peek().map(|tok| tok == end).unwrap_or(false) {
                self.next();
                self.skip_newlines();
                break;
            }

            match end {
                SyntaxKind::KwIn => self.errors.push("Expected either ';' or 'in'".to_string()),
                _ => self
                    .errors
                    .push("Expected either ';' or 'then'".to_string()),
            }

            break;
        }

        self.is_let.pop();
    }

    /// Handle a binary operator expression (or just next)
    fn handle_operation(
        &mut self,
        first: bool,
        tokens: &[SyntaxKind],
        next: fn(&mut Self, bool) -> bool,
        check_newlines: bool,
    ) -> bool {
        let checkpoint = self.builder.checkpoint();
        if !next(self, first) {
            return false;
        }

        let mut success_peek = if !check_newlines {
            |this: &mut Self, tokens: &[SyntaxKind]| {
                this.peek().map(|t| tokens.contains(&t)).unwrap_or(false)
            }
        } else {
            |this: &mut Self, tokens: &[SyntaxKind]| {
                let mut i = 0;
                while this.peek_nth(i) == Some(SyntaxKind::NewLine) {
                    i += 1;
                }

                if this
                    .peek_nth(i)
                    .map(|t| tokens.contains(&t))
                    .unwrap_or(false)
                {
                    for _ in 0..i {
                        this.next(); // skip newlines
                    }

                    true
                } else {
                    false
                }
            }
        };

        while success_peek(self, tokens) {
            self.builder
                .start_node_at(checkpoint, SyntaxKind::BiOp.into());
            self.bump();
            self.skip_newlines();
            next(self, false);
            self.builder.finish_node();
        }

        true
    }

    fn parse_period(&mut self, first: bool, next: fn(&mut Self, bool) -> bool) -> bool {
        self.handle_operation(first, &[SyntaxKind::OpPeriod], next, false)
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
            Self::parse_add,
            false,
        )
    }

    fn parse_pow(&mut self, first: bool) -> bool {
        self.handle_operation(first, &[SyntaxKind::OpPow], Self::parse_call, false)
    }

    fn parse_mul(&mut self, first: bool) -> bool {
        self.handle_operation(
            first,
            &[SyntaxKind::OpMul, SyntaxKind::OpDiv],
            Self::parse_pow,
            false,
        )
    }

    fn parse_add(&mut self, first: bool) -> bool {
        self.handle_operation(
            first,
            &[SyntaxKind::OpAdd, SyntaxKind::OpSub],
            Self::parse_mul,
            false,
        )
    }

    fn parse_pipe(&mut self, first: bool) -> bool {
        self.handle_operation(first, &[SyntaxKind::OpPipe], Self::parse_cmp, true)
    }

    fn parse_tuple(&mut self, first: bool) -> bool {
        if self.is_tuple() {
            self.handle_operation(first, &[SyntaxKind::OpComma], Self::parse_pipe, false)
        } else {
            self.parse_pipe(first)
        }
    }

    fn parse_asg(&mut self, first: bool) -> bool {
        self.handle_operation(first, &[SyntaxKind::OpAsg], Self::parse_tuple, false)
    }

    fn parse_expr(&mut self, first: bool) -> bool {
        let checkpoint = self.builder.checkpoint();
        let mut one_success = false;
        let mut exprs_cnt = 0;
        loop {
            if !self.parse_asg(first) {
                break;
            }

            one_success = true;

            if !self.is_let.is_empty() {
                break;
            }

            exprs_cnt += 1;
            if first && self.peek() == Some(SyntaxKind::Semicolon) {
                self.next();
                self.skip_newlines();
            } else {
                break;
            }
        }

        if exprs_cnt > 1 {
            self.builder
                .start_node_at(checkpoint, SyntaxKind::Root.into());
            self.builder.finish_node();
        }

        one_success
    }

    fn skip_newlines(&mut self) -> usize {
        let mut lines = 0;
        while self.peek() == Some(SyntaxKind::NewLine) {
            self.next();

            lines += 1;
            self.line += 1;
        }

        lines
    }

    pub fn parse(self) -> (Box<SyntaxElement>, Vec<String>) {
        self.parse_main(false)
    }

    pub fn parse_main(mut self, print_error_lines: bool) -> (Box<SyntaxElement>, Vec<String>) {
        self.builder.start_node(SyntaxKind::Root.into());
        self.line += self.skip_newlines();

        if !self.parse_expr(true) && print_error_lines {
            warn!("Error in line {}", self.line);
        }

        while self.peek() == Some(SyntaxKind::NewLine) {
            self.next();
            self.line += 1;

            let peeked = self.peek();
            if peeked.is_some()
                && peeked != Some(SyntaxKind::NewLine)
                && !self.parse_expr(true)
                && print_error_lines
            {
                warn!("Error in line {}", self.line);
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
        let kind: SyntaxKind = self.kind();
        match self {
            NodeOrToken::Node(node) => {
                let mut children = node.children_with_tokens();

                let expected_expr = InterpreterError::ExpectedExpression;

                match kind {
                    SyntaxKind::Root => {
                        let children: Vec<SyntaxElement> = children.collect();
                        if children.is_empty() {
                            Err(InterpreterError::InternalError(
                                "Root must have at least one child".to_string(),
                            ))
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
                    SyntaxKind::FnOp => {
                        let op = children
                            .next()
                            .and_then(|x| {
                                if let NodeOrToken::Token(tok) = x {
                                    Some(tok)
                                } else {
                                    None
                                }
                            })
                            .ok_or(InterpreterError::ExpectedOperator())?;

                        use crate::execute::BiOpType::*;

                        Ok(Syntax::FnOp(
                            (match op.kind() {
                                SyntaxKind::OpEq => Ok(OpEq),
                                SyntaxKind::OpNeq => Ok(OpNeq),
                                SyntaxKind::OpStrictEq => Ok(OpStrictEq),
                                SyntaxKind::OpStrictNeq => Ok(OpStrictNeq),
                                SyntaxKind::OpPow => Ok(OpPow),
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
                                SyntaxKind::OpComma => Ok(OpComma),
                                _ => Err(InterpreterError::InvalidOperator()),
                            })?,
                        ))
                    }
                    SyntaxKind::ExplicitExpr => {
                        match children
                            .next() {
                            Some(child) => Ok(Syntax::ExplicitExpr(Box::new(child.try_into()?))),
                            None => Ok(Syntax::EmptyTuple())
                        }
                    }
                    SyntaxKind::Lambda => {
                        let id = children
                            .next()
                            .and_then(|x| {
                                if let NodeOrToken::Token(tok) = x {
                                    Some(tok)
                                } else {
                                    None
                                }
                            })
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
                        let asgs: Vec<(SyntaxElement, SyntaxElement)> = children
                            [..children.len() - 1]
                            .iter()
                            .map(|child| {
                                if let NodeOrToken::Node(node) = child {
                                    let mut children = node.children_with_tokens();
                                    let lhs = children.next().ok_or(expected_expr())?;
                                    children.next();
                                    let rhs = children.next().ok_or(expected_expr())?;

                                    Ok((lhs, rhs))
                                } else {
                                    panic!("Not allowed here!");
                                }
                            })
                            .try_collect()?;

                        fn build_let(
                            asgs: &[(SyntaxElement, SyntaxElement)],
                            expr: SyntaxElement,
                        ) -> Result<Syntax, InterpreterError> {
                            if asgs.is_empty() {
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
                    SyntaxKind::KwMatch => {
                        let expr = children.next();
                        if expr.is_none() {
                            return Err(InterpreterError::ExpectedExpression());
                        }

                        let expr = expr.unwrap().try_into()?;
                        let mut children = children.map(|expr| expr.try_into());
                        let mut chunks = Vec::new();
                        while let (Some(lhs), Some(rhs)) = (children.next(), children.next()) {
                            chunks.push((lhs?, rhs?));
                        }

                        if !chunks.is_empty() {
                            Ok(Syntax::Match(Box::new(expr), chunks))
                        } else {
                            Err(InterpreterError::ExpectedMatchCase())
                        }
                    }
                    SyntaxKind::UnOp => {
                        let op = children
                            .next()
                            .and_then(|x| {
                                if let NodeOrToken::Token(tok) = x {
                                    Some(tok)
                                } else {
                                    None
                                }
                            })
                            .ok_or(InterpreterError::ExpectedOperator())?;
                        let expr = children.next().ok_or(expected_expr())?;

                        Ok(Syntax::UnOp(
                            match op.kind() {
                                SyntaxKind::OpImmediate => UnOpType::OpImmediate,
                                _ => panic!("Expected correct unary operator"),
                            },
                            Box::new(expr.try_into()?),
                        ))
                    }
                    SyntaxKind::BiOp => {
                        let lhs = children.next().ok_or(expected_expr())?;
                        let op = children
                            .next()
                            .and_then(|x| {
                                if let NodeOrToken::Token(tok) = x {
                                    Some(tok)
                                } else {
                                    None
                                }
                            })
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
                                    SyntaxKind::OpPow => Ok(OpPow),
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
                        let asgs: Vec<(Syntax, Syntax)> = children[..children.len() - 2]
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
                        let mut children = children;
                        while let Some(NodeOrToken::Node(map_element)) = children.next() {
                            let mut children = map_element.children_with_tokens();
                            let id = children.next().unwrap().into_token().unwrap();
                            let val: SyntaxElement = children.next().unwrap();

                            map.push((
                                id.text().to_string(),
                                (val.try_into()?, {
                                    let id: SyntaxKind = id.kind();
                                    id == SyntaxKind::Id
                                }),
                            ));
                        }

                        Ok(Syntax::Map(map.into_iter().collect()))
                    }
                    SyntaxKind::MapMatch => {
                        let mut map = Vec::new();
                        let mut children = children;
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

                            let val = children.next().map(|x| x.try_into()).transpose()?;
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
                SyntaxKind::Rg => {
                    let re = Regex::new(tok.text())
                        .map_err(|err| InterpreterError::InvalidRegex(err.to_string()))?;

                    Ok(Syntax::ValRg(tok.text().to_string()))
                }
                SyntaxKind::Atom => Ok(Syntax::ValAtom(tok.text().to_string())),
                SyntaxKind::Any => Ok(Syntax::ValAny()),
                _ => Err(InterpreterError::UnexpectedTokenEmpty()),
            },
        }
    }
}

pub fn print_ast(indent: usize, element: &SyntaxElement) {
    let kind: SyntaxKind = element.kind();
    print!("{:indent$}", "", indent = indent);
    match element {
        NodeOrToken::Node(node) => {
            println!("- {kind:?}");
            for child in node.children_with_tokens() {
                print_ast(indent + 2, &child);
            }
        }

        NodeOrToken::Token(token) => println!("- {:?} {kind:?}", token.text()),
    }
}
