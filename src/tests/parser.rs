use rowan::GreenNodeBuilder;

use crate::{
    errors::InterpreterError,
    execute::Syntax,
    lexer::Token,
    parser::{Parser, SyntaxElement, SyntaxKind},
};

async fn parse(line: &str) -> Result<Syntax, InterpreterError> {
    let toks = Token::lex_for_rowan(line);
    let toks: Vec<(SyntaxKind, String)> = toks
        .into_iter()
        .map(
            |(tok, slice)| -> Result<(SyntaxKind, String), InterpreterError> {
                Ok((tok.clone().try_into()?, slice.clone()))
            },
        )
        .try_collect()?;

    let (ast, errors) = Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
    if !errors.is_empty() {
        Err(InterpreterError::UnknownError())
    } else {
        (*ast).try_into()
    }
}

async fn parse_to_str(line: &str) -> Result<String, InterpreterError> {
    let typed = parse(line).await?;
    Ok(format!("{}", typed))
}

#[tokio::test]
async fn parse_lambda() {
    assert_eq!(Ok(format!("(\\x x)")), parse_to_str("\\x x").await);
    assert_eq!(
        Ok(format!("(\\x (x + y))")),
        parse_to_str("\\x x + y").await
    );
}

#[tokio::test]
async fn parse_let() {
    assert_eq!(
        Ok(format!("(let x = 0 in x)")),
        parse_to_str("let x = 0 in x").await
    );
    assert_eq!(
        Ok(format!("(let x = 0 in (let y = 1 in x))")),
        parse_to_str("let x = 0; y = 1 in x").await
    );
}

#[tokio::test]
async fn parse_map() {
    assert_eq!(Ok(r"{ x }".to_string()), parse_to_str("{x}").await);
    assert_eq!(
        Ok("(let { x } = { x: \"Hello, world\" } in x)".to_string()),
        parse_to_str("let {x} = {x: \"Hello, world\"} in x").await
    );
}

#[tokio::test]
async fn parse_pipe_op() {
    assert_eq!(
        Ok(r"(1 | (\x (x + 2)))".to_string()),
        parse_to_str(r"1 | \x x + 2").await
    );
}

#[tokio::test]
async fn parse_negative() {
    assert_eq!(Ok(r"(0 - 1)".to_string()), parse_to_str("-1").await);
}

#[tokio::test]
async fn parse_std_right_expr() {
    assert_eq!(
        Ok(r"((std.right) ((1 2) 3))".to_string()),
        parse_to_str("std.right (1 2 3)").await
    );
}

#[tokio::test]
async fn parse_biop() {
    assert_eq!(Ok(r"(x == y)".to_string()), parse_to_str("x == y").await);
    assert_eq!(Ok(r"(x.y)".to_string()), parse_to_str("x.y").await);
    assert_eq!(Ok(r"(x - y)".to_string()), parse_to_str("x - y").await);
    assert_eq!(Ok(r"(x + y)".to_string()), parse_to_str("x + y").await);
    assert_eq!(Ok(r"(x * y)".to_string()), parse_to_str("x * y").await);
    assert_eq!(Ok(r"(x / y)".to_string()), parse_to_str("x / y").await);
    assert_eq!(Ok(r"(x != y)".to_string()), parse_to_str("x != y").await);
    assert_eq!(Ok(r"(x === y)".to_string()), parse_to_str("x === y").await);
    assert_eq!(Ok(r"(x !== y)".to_string()), parse_to_str("x !== y").await);
    assert_eq!(Ok(r"(x >= y)".to_string()), parse_to_str("x >= y").await);
    assert_eq!(Ok(r"(x <= y)".to_string()), parse_to_str("x <= y").await);
    assert_eq!(Ok(r"(x > y)".to_string()), parse_to_str("x > y").await);
    assert_eq!(Ok(r"(x < y)".to_string()), parse_to_str("x < y").await);
    assert_eq!(Ok(r"(x | y)".to_string()), parse_to_str("x | y").await);
}

#[tokio::test]
async fn parse_str() {
    assert_eq!(Ok("\"test\"".to_string()), parse_to_str("\"test\"").await);
    assert_eq!(Ok("\"\\n\"".to_string()), parse_to_str("\"\\n\"").await);
    assert_eq!(
        Ok("\"\\t\\n\"".to_string()),
        parse_to_str("\"\\t\\n\"").await
    );
    assert_eq!(
        Ok("\"\\t\\n\"".to_string()),
        parse_to_str("\"\\t\\n\"").await
    );
    assert_eq!(
        Ok("\"\\r\\t\\n\"".to_string()),
        parse_to_str("\"\\r\\t\\n\"").await
    );
    assert_eq!(
        Ok("\"\\0\\r\\t\\n\"".to_string()),
        parse_to_str("\"\\0\\r\\t\\n\"").await
    );
    assert_eq!(
        Ok("\"\\\\0\\r\\t\\n\"".to_string()),
        parse_to_str("\"\\\\\\0\\r\\t\\n\"").await
    );
    assert_eq!(
        Ok("\"\\\"\\\\0\\r\\t\\n\"".to_string()),
        parse_to_str("\"\\\"\\\\\\0\\r\\t\\n\"").await
    );
    assert_eq!(
        Ok("\"\'\\\"\\\\0\\r\\t\\n\"".to_string()),
        parse_to_str("\"\\\'\\\"\\\\\\0\\r\\t\\n\"").await
    );
}
