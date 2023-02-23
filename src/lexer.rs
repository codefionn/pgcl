///! This module is for transforming a PGCL code/line into tokens
use logos::Logos;
use num::{BigRational, Num};

use crate::{errors::InterpreterError, parser::SyntaxKind};

/// PGCL tokens
#[derive(Logos, Clone, Debug, PartialEq)]
#[repr(u16)]
pub enum Token {
    #[token("\\")]
    Lambda,

    #[token("(")]
    ParenLeft,

    #[token(")")]
    ParenRight,

    #[token("[")]
    LstLeft,

    #[token("]")]
    LstRight,

    #[token("{")]
    MapLeft,

    #[token("}")]
    MapRight,

    #[token("+")]
    OpAdd,

    #[token("-")]
    OpSub,

    #[token("*")]
    OpMul,

    #[token("/")]
    OpDiv,

    #[token("..")]
    Unpack,

    #[token(".")]
    OpPeriod,

    #[token(",")]
    OpComma,

    #[token(">=")]
    OpGeq,

    #[token("<=")]
    OpLeq,

    #[token(">")]
    OpGt,

    #[token("<")]
    OpLt,

    #[token("==")]
    OpEq,

    #[token("===")]
    OpStrictEq,

    #[token("=")]
    OpAsg,

    #[token("!=")]
    OpNeq,

    #[token("!==")]
    OpStrictNeq,

    #[token(":")]
    OpMap,

    #[token(";")]
    Semicolon,

    #[regex(r"[0-9]*\.[0-9]+", |lex| lex.slice().to_string())]
    Flt(String),

    #[regex(r"[0-9]+", |lex| dec_to_big_rational(lex.slice()))]
    #[regex(r"0x[0-9A-Fa-f]+", |lex| hex_to_big_rational(lex.slice()))]
    Int(num::BigRational),

    #[token("if")]
    KwIf,

    #[token("then")]
    KwThen,

    #[token("else")]
    KwElse,

    #[token("let")]
    KwLet,

    #[token("in")]
    KwIn,

    #[token("_")]
    Any,

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Id(String),

    #[regex(r"@[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
    Atom(String),

    // Yeah, understanding this regex in the future will be kinda hard
    #[regex(r"\u{0022}([^\u{0022}\\]|\\([rnt\u{0022}\\']|u\{[A-F0-9][A-F0-9]?[A-F0-9]?[A-F0-9]?[A-F0-9]?[A-F0-9]?[A-F0-9]?[A-F0-9]?\}))*\u{0022}", |lex| parse_string(lex.slice()))]
    Str(String),

    #[token("\n\r")]
    #[token("\r\n")]
    #[token("\n")]
    #[token("\r")]
    NewLine,

    #[error]
    #[regex(r"[ \t\f]+", logos::skip)]
    #[regex(r"//[^\n\r]*", logos::skip)]
    Error,
}

impl Token {
    pub fn lex_for_rowan<'a>(text: &'a str) -> Vec<(Token, String)> {
        let mut lex = Token::lexer(text);
        let mut result = Vec::new();

        while let Some(tok) = lex.next() {
            use Token::*;
            let slice = match tok.clone() {
                Lambda | ParenLeft | ParenRight | LstLeft | LstRight | MapLeft | MapRight
                | OpAdd | OpSub | OpMul | OpDiv | Unpack | OpPeriod | OpComma | OpAsg | OpEq
                | OpStrictEq | OpNeq | OpStrictNeq | OpMap | KwIn | KwLet | NewLine | Semicolon
                | Any | OpLeq | OpGeq | OpGt | OpLt | KwIf | KwElse | KwThen | Error => {
                    lex.slice().to_string()
                }

                Flt(x) => x.to_string(),
                Int(x) => x.to_string(),
                Id(x) => x.clone(),
                Atom(x) => x.clone(),
                Str(x) => x.clone(),
            };
            result.push((tok, slice));
        }

        result
    }
}

impl TryInto<SyntaxKind> for Token {
    type Error = InterpreterError;
    fn try_into(self) -> Result<SyntaxKind, InterpreterError> {
        match self {
            Token::Lambda => Ok(SyntaxKind::Lambda),
            Token::ParenLeft => Ok(SyntaxKind::ParenLeft),
            Token::ParenRight => Ok(SyntaxKind::ParenRight),
            Token::LstLeft => Ok(SyntaxKind::LstLeft),
            Token::LstRight => Ok(SyntaxKind::LstRight),
            Token::MapLeft => Ok(SyntaxKind::MapLeft),
            Token::MapRight => Ok(SyntaxKind::MapRight),
            Token::OpAdd => Ok(SyntaxKind::OpAdd),
            Token::OpSub => Ok(SyntaxKind::OpSub),
            Token::OpMul => Ok(SyntaxKind::OpMul),
            Token::OpDiv => Ok(SyntaxKind::OpDiv),
            Token::Unpack => Ok(SyntaxKind::Unpack),
            Token::OpPeriod => Ok(SyntaxKind::OpPeriod),
            Token::OpComma => Ok(SyntaxKind::OpComma),
            Token::OpAsg => Ok(SyntaxKind::OpAsg),
            Token::OpEq => Ok(SyntaxKind::OpEq),
            Token::OpStrictEq => Ok(SyntaxKind::OpStrictEq),
            Token::OpNeq => Ok(SyntaxKind::OpNeq),
            Token::OpStrictNeq => Ok(SyntaxKind::OpStrictNeq),
            Token::OpGeq => Ok(SyntaxKind::OpGeq),
            Token::OpLeq => Ok(SyntaxKind::OpLeq),
            Token::OpGt => Ok(SyntaxKind::OpGt),
            Token::OpLt => Ok(SyntaxKind::OpLt),
            Token::OpMap => Ok(SyntaxKind::OpMap),
            Token::Semicolon => Ok(SyntaxKind::Semicolon),
            Token::KwIf => Ok(SyntaxKind::If),
            Token::KwThen => Ok(SyntaxKind::KwThen),
            Token::KwElse => Ok(SyntaxKind::KwElse),
            Token::Flt(_) => Ok(SyntaxKind::Flt),
            Token::Int(_) => Ok(SyntaxKind::Int),
            Token::KwLet => Ok(SyntaxKind::KwLet),
            Token::KwIn => Ok(SyntaxKind::KwIn),
            Token::Any => Ok(SyntaxKind::Any),
            Token::Id(_) => Ok(SyntaxKind::Id),
            Token::Atom(_) => Ok(SyntaxKind::Atom),
            Token::Str(_) => Ok(SyntaxKind::Str),
            Token::Error => Err(InterpreterError::UnknownError()),
            tok @ _ => Err(InterpreterError::UnexpectedToken(tok)),
        }
    }
}

#[inline]
fn dec_to_big_rational(num: &str) -> num::BigRational {
    num.parse().unwrap()
}

#[inline]
fn hex_to_big_rational(num: &str) -> Result<num::BigRational, num::rational::ParseRatioError> {
    num::BigRational::from_str_radix(&num[2..], 16)
}

#[inline]
fn parse_string(mystr: &str) -> Option<String> {
    let mystr = &mystr[1..mystr.len() - 1];

    let mut result = String::with_capacity(mystr.len());
    let mut idx = 0;
    let chars: Vec<char> = mystr.chars().collect();
    while let Some(idx_bsl) = mystr.get(idx..).map(|mystr| mystr.find("\\")).flatten() {
        println!("{}, {}", idx, idx_bsl);
        result += &mystr[idx..(idx + idx_bsl)];
        match chars.get(idx + idx_bsl + 1) {
            Some('n') => result += "\n",
            Some('r') => result += "\r",
            Some('t') => result += "\t",
            Some('0') => result += "\0",
            Some('\\') => result += "\\",
            Some('\"') => result += "\"",
            Some('\'') => result += "\'",
            _ => {
                println!("{}", mystr);
                return None;
            }
        }

        idx += idx_bsl + 2;
        println!("=> {}, {}", idx, idx_bsl);
    }

    if idx < mystr.len() {
        result += &mystr[idx..];
    }

    Some(result)
}
