use crate::{lexer::Token, parser::SyntaxKind};

#[derive(Clone, Debug, PartialEq)]
pub enum InterpreterError {
    UnknownError(),
    InternalError(String),
    GlobalNotInContext(String, String),
    UnexpectedToken(Token),
    UnexpectedTokenEmpty(),
    UnexpectedExpressionEmpty(SyntaxKind),
    ExpectedExpression(),
    ExpectedMatchCase(),
    NoMatchCaseMatched(),
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
    ExpectedContext(usize),
    ExpectedSytem(usize),
    CouldNotCreateRunner(Option<String>),
    ImportFileDoesNotExist(String),
    InvalidEscapeSequence(/* seq: */ String, /* subj: */ String),
    NumberTooBig(),
    InvalidRegex(String),
    ProgramTerminatedByUser(i32),
}

impl Default for InterpreterError {
    fn default() -> Self {
        Self::UnknownError()
    }
}
