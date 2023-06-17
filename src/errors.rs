use crate::{lexer::Token, parser::SyntaxKind};

#[derive(Debug, PartialEq)]
pub enum InterpreterError {
    UnknownError(),
    InternalError(String),
    GlobalNotInContext(String, String),
    UnexpectedToken(Token),
    UnexpectedTokenEmpty(),
    UnexpectedExpressionEmpty(SyntaxKind),
    ExpectedExpression(),
    ExpectedRHSExpression(),
    ExpectedLHSExpression(),
    ExpectedIdentifier(),
    ExpectedOperator(),
    InvalidOperator(),
    ExpectedToken(),
    ExpectedInteger(),
    ExpectedFloat(),
    ExpectedCall(),
    LetDoesMatch(String),
    ContextNotInFile(String),
    ImportFileDoesNotExist(String),
    NumberTooBig(),
    ProgramTerminatedByUser(i32),
}
