///! This module is for creating an untyped AST and creating an typed AST from it
use std::{iter::Peekable, str::FromStr};

use bevy::app::AppLabel;
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
    Any,
    Id,
    Atom,
    Str,

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
}

impl<I: Iterator<Item = (SyntaxKind, String)>> Parser<I> {
    pub fn new(builder: GreenNodeBuilder<'static>, iter: Peekable<I>) -> Self {
        Self {
            builder,
            iter,
            errors: Default::default(),
        }
    }
}

impl<I: Iterator<Item = (SyntaxKind, String)>> Parser<I> {
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
    fn parse_val(&mut self, allow_empty: bool) -> bool {
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
            Some(SyntaxKind::ParenLeft) => {
                self.iter.next(); // skip
                self.parse_expr();
                if self
                    .peek()
                    .map(|tok| tok == SyntaxKind::ParenRight)
                    .unwrap_or(false)
                {
                    self.iter.next(); // skip
                } else {
                    self.errors.push(format!("Expected )"));
                    self.builder.start_node(SyntaxKind::Error.into());
                    self.builder.finish_node();
                }

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
                    self.parse_expr();

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
                self.parse_expr();

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
                self.parse_expr();

                self.builder.finish_node();

                true
            }
            Some(SyntaxKind::KwLet) => {
                self.iter.next(); // Skip let

                self.builder.start_node(SyntaxKind::KwLet.into());
                self.parse_let_args(SyntaxKind::KwIn);
                self.parse_expr();
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
                self.parse_expr();
                self.builder.finish_node();

                true
            }
            _ => {
                if !allow_empty {
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
            self.parse_call();

            if self
                .peek()
                .map(|tok| tok == SyntaxKind::OpAsg)
                .unwrap_or(false)
            {
                self.bump();
            } else {
                self.errors.push(format!("Expected = in let expression"));
            }

            self.parse_expr();

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
    fn handle_operation(&mut self, tokens: &[SyntaxKind], next: fn(&mut Self) -> bool) -> bool {
        let checkpoint = self.builder.checkpoint();
        if !next(self) {
            return false;
        }

        while self.peek().map(|t| tokens.contains(&t)).unwrap_or(false) {
            self.builder
                .start_node_at(checkpoint, SyntaxKind::BiOp.into());
            self.bump();
            next(self);
            self.builder.finish_node();
        }

        true
    }

    fn parse_period(&mut self, next: fn(&mut Self) -> bool) -> bool {
        self.handle_operation(&[SyntaxKind::OpPeriod], next)
    }

    /// Parse a lambda/function call expression
    fn parse_call(&mut self) -> bool {
        let maybe_call = self.builder.checkpoint();
        if !self.parse_period(|this| this.parse_val(false)) {
            return false;
        }

        while self.parse_period(|this| this.parse_val(true)) {
            self.builder
                .start_node_at(maybe_call, SyntaxKind::Call.into());
            self.builder.finish_node();
        }

        true
    }

    fn parse_cmp(&mut self) -> bool {
        self.handle_operation(
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

    fn parse_mul(&mut self) -> bool {
        self.handle_operation(&[SyntaxKind::OpMul, SyntaxKind::OpDiv], Self::parse_cmp)
    }

    fn parse_add(&mut self) -> bool {
        self.handle_operation(&[SyntaxKind::OpAdd, SyntaxKind::OpSub], Self::parse_mul)
    }

    fn parse_tuple(&mut self) -> bool {
        self.handle_operation(&[SyntaxKind::OpComma], Self::parse_add)
    }

    fn parse_asg(&mut self) -> bool {
        self.handle_operation(&[SyntaxKind::OpAsg], Self::parse_tuple)
    }

    fn parse_expr(&mut self) {
        self.parse_asg();
    }

    pub fn parse(mut self) -> (SyntaxNode, Vec<String>) {
        self.builder.start_node(SyntaxKind::Root.into());
        self.parse_expr();
        self.builder.finish_node();

        (SyntaxNode::new_root(self.builder.finish()), self.errors)
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
                        if children.len() != 1 {
                            Err(InterpreterError::InternalError(format!(
                                "Root must only have one child"
                            )))
                        } else {
                            children.into_iter().next().unwrap().try_into()
                        }
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
                    _ => Err(InterpreterError::UnexpectedExpressionEmpty()),
                }
            }
            NodeOrToken::Token(tok) => match kind {
                SyntaxKind::Id => Ok(Syntax::Id(tok.text().to_string())),
                SyntaxKind::Int => Ok(Syntax::ValInt(
                    num::BigInt::from_str_radix(tok.text(), 10)
                        .map_err(|_| InterpreterError::ExpectedInteger())?,
                )),
                SyntaxKind::Flt => Ok(Syntax::ValFlt(
                    BigDecimal::from_str(tok.text())
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

pub fn print_ast(indent: usize, element: SyntaxElement) {
    let kind: SyntaxKind = element.kind().into();
    print!("{:indent$}", "", indent = indent);
    match element {
        NodeOrToken::Node(node) => {
            println!("- {:?}", kind);
            for child in node.children_with_tokens() {
                print_ast(indent + 2, child);
            }
        }

        NodeOrToken::Token(token) => println!("- {:?} {:?}", token.text(), kind),
    }
}
