use rowan::GreenNodeBuilder;

use crate::{
    errors::InterpreterError,
    execute::{Context, Syntax},
    lexer::Token,
    parser::{Parser, SyntaxElement, SyntaxKind},
};
fn parse(line: &str, ctx: &mut Context) -> Result<Syntax, InterpreterError> {
    let toks = Token::lex_for_rowan(line);
    let toks: Vec<(SyntaxKind, String)> = toks
        .into_iter()
        .map(|(tok, slice)| (tok.clone().try_into().unwrap(), slice.clone()))
        .collect();

    let (ast, errors) = Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
    if !errors.is_empty() {
        Err(InterpreterError::UnknownError())
    } else {
        let ast: SyntaxElement = ast.into();
        let ast: Syntax = ast.try_into()?;
        ast.execute(true, ctx)
    }
}

fn parse_to_str(line: &str, ctx: &mut Context) -> Result<String, InterpreterError> {
    let typed = parse(line, ctx)?;
    Ok(format!("{}", typed))
}

#[test]
fn add_integers() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str("2 + 3", &mut Default::default())
    );
    assert_eq!(
        Ok(format!("11")),
        parse_to_str("2 + 3 * 3", &mut Default::default())
    );
}

#[test]
fn add_floats() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str("2.0 + 3.0", &mut Default::default())
    );
    assert_eq!(
        Ok(format!("7")),
        parse_to_str("2.5 + 4.5", &mut Default::default())
    );
}

#[test]
fn eq() {
    assert_eq!(
        Ok(format!(":true")),
        parse_to_str("2 == 2", &mut Default::default())
    );
    assert_eq!(
        Ok(format!(":false")),
        parse_to_str("3 == 2", &mut Default::default())
    );
}

#[test]
fn recursion() {
    let mut ctx = Context::default();
    assert!(parse_to_str("fib 1 = 1", &mut ctx).is_ok());
    assert!(parse_to_str("fib 2 = 1", &mut ctx).is_ok());
    assert!(parse_to_str("fib x = fib (x-2) + fib (x-1)", &mut ctx).is_ok());
    assert_eq!(Ok(format!("55")), parse_to_str("fib 10", &mut ctx));
}
