use crate::lexer::Token;

#[derive(Debug, PartialEq)]
pub enum InterpreterError {
    UnknownError(),
    InternalError(String),
    UnexpectedToken(Token),
    UnexpectedTokenEmpty(),
    UnexpectedExpressionEmpty(),
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
}
