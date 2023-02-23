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
        Ok(format!("@true")),
        parse_to_str("2 == 2", &mut Default::default())
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str("3 == 2", &mut Default::default())
    );
}

#[test]
fn add_str() {
    assert_eq!(
        Ok(format!("\"helloworld\"")),
        parse_to_str("\"hello\" + \"world\"", &mut Default::default())
    );
}

#[test]
fn neq() {
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str("2 != 2", &mut Default::default())
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str("3 != 2", &mut Default::default())
    );
}

#[test]
fn fn_add_lambda() {
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"(\x = \y = x + y) 2 10", &mut Default::default())
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"(\x = \y = x + y) 10 2", &mut Default::default())
    );
}

#[test]
fn fn_add_fn_and_lambda() {
    let mut ctx = Context::default();
    assert!(parse_to_str(r"add x = \y = x + y", &mut ctx).is_ok());
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 10 2", &mut ctx));
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 2 10", &mut ctx));
}

#[test]
fn fn_add_fn() {
    let mut ctx = Context::default();
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).is_ok());
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 10 2", &mut ctx));
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 2 10", &mut ctx));
}

#[test]
fn fn_add_fn_tuple() {
    let mut ctx = Context::default();
    assert!(parse_to_str(r"add (x, y) = x + y", &mut ctx).is_ok());
    assert_eq!(Ok(format!("12")), parse_to_str(r"add (10, 2)", &mut ctx));
    assert_eq!(Ok(format!("12")), parse_to_str(r"add (2, 10)", &mut ctx));
}

#[test]
fn op_geq() {
    let mut ctx = Context::default();
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).is_ok());
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 2", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 3", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 1", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"1 >= 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"2 >= 5", &mut ctx));

    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 2.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 3.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 0.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 1.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 >= 5.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"1 >= 5.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"2 >= 5.0", &mut ctx));

    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 >= 2", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 >= 3", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 >= 0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 >= 1", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 >= 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"1.0 >= 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"2.0 >= 5", &mut ctx));

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 4) >= 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 3) >= 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 2) >= 5", &mut ctx)
    );
}

#[test]
fn op_gt() {
    let mut ctx = Context::default();
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).is_ok());
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 2", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 3", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 1", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 > 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"1 > 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"2 > 5", &mut ctx));

    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 2.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 3.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 0.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 1.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 > 5.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"1 > 5.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"2 > 5.0", &mut ctx));

    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 > 2", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 > 3", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 > 0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 > 1", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 > 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"1.0 > 5", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"2.0 > 5", &mut ctx));

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 4) > 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 3) > 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 2) > 5", &mut ctx)
    );
}

#[test]
fn op_leq() {
    let mut ctx = Context::default();
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).is_ok());
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 2", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 3", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 1", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 <= 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"1 <= 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"2 <= 5", &mut ctx));

    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 2.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 3.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 0.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 <= 1.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 <= 5.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"1 <= 5.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"2 <= 5.0", &mut ctx));

    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 <= 2", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 <= 3", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 <= 0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 <= 1", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5.0 <= 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"1.0 <= 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"2.0 <= 5", &mut ctx));

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 4) <= 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 3) <= 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 2) <= 5", &mut ctx)
    );
}

#[test]
fn op_lt() {
    let mut ctx = Context::default();
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).is_ok());
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 2", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 3", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 1", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"1 < 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"2 < 5", &mut ctx));

    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 2.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 3.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 0.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 1.0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5 < 5.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"1 < 5.0", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"2 < 5.0", &mut ctx));

    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 < 2", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 < 3", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 < 0", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 < 1", &mut ctx));
    assert_eq!(Ok(format!("@false")), parse_to_str(r"5.0 < 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"1.0 < 5", &mut ctx));
    assert_eq!(Ok(format!("@true")), parse_to_str(r"2.0 < 5", &mut ctx));

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 4) < 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 3) < 5", &mut ctx)
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 2) < 5", &mut ctx)
    );
}

#[test]
fn recursion_fib() {
    let mut ctx = Context::default();
    assert!(parse_to_str("fib 1 = 1", &mut ctx).is_ok());
    assert!(parse_to_str("fib 2 = 1", &mut ctx).is_ok());
    assert!(parse_to_str("fib x = fib (x-2) + fib (x-1)", &mut ctx).is_ok());
    assert_eq!(Ok(format!("55")), parse_to_str("fib 10", &mut ctx));
}

#[test]
fn pattern_match_list() {
    let mut ctx = Context::default();
    assert!(parse_to_str("len [] = 0", &mut ctx).is_ok());
    assert!(parse_to_str("len [x;xs] = 1 + len xs", &mut ctx).is_ok());
    assert_eq!(Ok(format!("0")), parse_to_str("len []", &mut ctx));
    assert_eq!(Ok(format!("1")), parse_to_str("len [0]", &mut ctx));
    assert_eq!(Ok(format!("2")), parse_to_str("len [0, 1]", &mut ctx));
    assert_eq!(Ok(format!("3")), parse_to_str("len [0, 1, 2]", &mut ctx));
}

#[test]
fn test_map() {
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x} = {x: \"Hello, world\"} in x",
            &mut Context::default()
        )
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x} = {x: \"Hello, world\", y: 0} in x",
            &mut Context::default()
        )
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x} = {y: 0, x: \"Hello, world\"} in x",
            &mut Context::default()
        )
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\" a} = {y: 0, x: \"Hello, world\"} in a",
            &mut Context::default()
        )
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\": y} = {y: 0, x: \"Hello, world\"} in y",
            &mut Context::default()
        )
    );
    assert_eq!(
        Ok("10".to_string()),
        parse_to_str(
            "if let {\"z\"} = {y: 0, x: \"Hello, world\"} then z else 10",
            &mut Context::default()
        )
    );
}
