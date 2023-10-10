use crate::{lexer::Token, parser::SyntaxKind};

#[derive(Clone, Debug, PartialEq)]
pub enum InterpreterError {
    UnknownError(),
    UnknownSymbol(),
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

#[derive(Clone, Debug, PartialEq)]
pub enum LexerError {
    UnknownSymbol(),
    NumberTooBig(),
    UnexpectedToken(Token),
    InvalidRegex(String),
    InvalidEscapeSequence(String, String),
}

impl Default for LexerError {
    fn default() -> Self {
        Self::UnknownSymbol()
    }
}

impl Into<InterpreterError> for LexerError {
    fn into(self) -> InterpreterError {
        match self {
            Self::UnknownSymbol() => InterpreterError::UnknownSymbol(),
            Self::InvalidRegex(msg) => InterpreterError::InvalidRegex(msg),
            Self::UnexpectedToken(tok) => InterpreterError::UnexpectedToken(tok),
            Self::InvalidEscapeSequence(c, s) => InterpreterError::InvalidEscapeSequence(c, s),
            Self::NumberTooBig() => InterpreterError::NumberTooBig(),
        }
    }
}
