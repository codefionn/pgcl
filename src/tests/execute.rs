use rowan::GreenNodeBuilder;

use crate::{
    context::{Context, ContextHandler},
    errors::InterpreterError,
    execute::Syntax,
    lexer::Token,
    parser::{Parser, SyntaxElement, SyntaxKind},
};

async fn parse(line: &str, ctx: &mut ContextHandler) -> Result<Syntax, InterpreterError> {
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
        let ast: Syntax = (*ast).try_into()?;
        ast.execute(true, ctx).await
    }
}

async fn parse_to_str(line: &str, ctx: &mut ContextHandler) -> Result<String, InterpreterError> {
    let typed = parse(line, ctx).await?;
    Ok(format!("{}", typed))
}

#[tokio::test]
async fn add_integers() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str("2 + 3", &mut ContextHandler::async_default().await).await
    );
    assert_eq!(
        Ok(format!("11")),
        parse_to_str("2 + 3 * 3", &mut ContextHandler::async_default().await).await
    );
}

#[tokio::test]
async fn add_with_pipe() {
    assert_eq!(
        Ok(r"3".to_string()),
        parse_to_str(r"1 | \x x + 2", &mut ContextHandler::async_default().await).await
    );
}

#[tokio::test]
async fn add_floats() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str("2.0 + 3.0", &mut ContextHandler::async_default().await).await
    );
    assert_eq!(
        Ok(format!("7")),
        parse_to_str("2.5 + 4.5", &mut ContextHandler::async_default().await).await
    );
}

#[tokio::test]
async fn eq() {
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str("2 == 2", &mut ContextHandler::async_default().await).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str("3 == 2", &mut ContextHandler::async_default().await).await
    );
}

#[tokio::test]
async fn add_str() {
    assert_eq!(
        Ok(format!("\"helloworld\"")),
        parse_to_str(
            "\"hello\" + \"world\"",
            &mut ContextHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn neq() {
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str("2 != 2", &mut ContextHandler::async_default().await).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str("3 != 2", &mut ContextHandler::async_default().await).await
    );
}

#[tokio::test]
async fn fn_add_lambda() {
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = \y = x + y) 2 10",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = \y = x + y) 10 2",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = (\y x + y)) 10 2",
            &mut ContextHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn fn_add_fn_and_lambda() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str(r"add x = \y = x + y", &mut ctx).await.is_ok());
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 10 2", &mut ctx).await);
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 2 10", &mut ctx).await);
}

#[tokio::test]
async fn fn_add_fn() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).await.is_ok());
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 10 2", &mut ctx).await);
    assert_eq!(Ok(format!("12")), parse_to_str(r"add 2 10", &mut ctx).await);
}

#[tokio::test]
async fn fn_add_fn_tuple() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str(r"add (x, y) = x + y", &mut ctx).await.is_ok());
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add (10, 2)", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add (2, 10)", &mut ctx).await
    );
}

#[tokio::test]
async fn test_fn_atom() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str("add @zero y = y", &mut ctx).await.is_ok());
    assert!(parse_to_str("add (@succ x) y = add x (@succ y)", &mut ctx)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!(r"@zero")),
        parse_to_str(r"add @zero @zero", &mut ctx).await
    );
    assert_eq!(
        Ok(format!(r"(@succ @zero)")),
        parse_to_str(r"add @zero (@succ @zero)", &mut ctx).await
    );
    assert_eq!(
        Ok(format!(r"(@succ (@succ @zero))")),
        parse_to_str(r"add @zero (@succ (@succ @zero))", &mut ctx).await
    );
    assert_eq!(
        Ok(format!(r"(@succ (@succ (@succ @zero)))")),
        parse_to_str(r"add (@succ @zero) (@succ (@succ @zero))", &mut ctx).await
    );
    assert_eq!(
        Ok(format!(r"(@succ (@succ (@succ (@succ (@succ @zero)))))")),
        parse_to_str(
            r"add (@succ (@succ (@succ @zero))) (@succ (@succ @zero))",
            &mut ctx
        )
        .await
    );
}

#[tokio::test]
async fn op_geq() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).await.is_ok());
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 2", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 3", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 1", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 >= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 >= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 2.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 3.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 0.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 1.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 >= 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 >= 5.0", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 2", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 3", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 1", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1.0 >= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2.0 >= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 4) >= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 3) >= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 2) >= 5", &mut ctx).await
    );
}

#[tokio::test]
async fn op_gt() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).await.is_ok());
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 2", &mut ctx).await);
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 3", &mut ctx).await);
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 0", &mut ctx).await);
    assert_eq!(Ok(format!("@true")), parse_to_str(r"5 > 1", &mut ctx).await);
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 > 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 > 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 > 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 2.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 3.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 0.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 1.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 > 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 > 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 > 5.0", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 2", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 3", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 1", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 > 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1.0 > 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2.0 > 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 4) > 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 3) > 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 2) > 5", &mut ctx).await
    );
}

#[tokio::test]
async fn op_leq() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).await.is_ok());
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 2", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 3", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 1", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 <= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1 <= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2 <= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 2.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 3.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 0.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 1.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 <= 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1 <= 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2 <= 5.0", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 2", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 3", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 1", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 <= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1.0 <= 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2.0 <= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 4) <= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 3) <= 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 2) <= 5", &mut ctx).await
    );
}

#[tokio::test]
async fn op_lt() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx).await.is_ok());
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 2", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 3", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 1", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 5", &mut ctx).await
    );
    assert_eq!(Ok(format!("@true")), parse_to_str(r"1 < 5", &mut ctx).await);
    assert_eq!(Ok(format!("@true")), parse_to_str(r"2 < 5", &mut ctx).await);

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 2.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 3.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 0.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 1.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1 < 5.0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2 < 5.0", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 2", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 3", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 0", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 1", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1.0 < 5", &mut ctx).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2.0 < 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 4) < 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 3) < 5", &mut ctx).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 2) < 5", &mut ctx).await
    );
}

#[tokio::test]
async fn recursion_fib() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str("fib 1 = 1", &mut ctx).await.is_ok());
    assert!(parse_to_str("fib 2 = 1", &mut ctx).await.is_ok());
    assert!(parse_to_str("fib x = fib (x-2) + fib (x-1)", &mut ctx)
        .await
        .is_ok());
    assert_eq!(Ok(format!("55")), parse_to_str("fib 10", &mut ctx).await);
}

#[tokio::test]
async fn pattern_match_list() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str("len [] = 0", &mut ctx).await.is_ok());
    assert!(parse_to_str("len [x;xs] = 1 + len xs", &mut ctx)
        .await
        .is_ok());
    assert_eq!(Ok(format!("0")), parse_to_str("len []", &mut ctx).await);
    assert_eq!(Ok(format!("1")), parse_to_str("len [0]", &mut ctx).await);
    assert_eq!(Ok(format!("2")), parse_to_str("len [0, 1]", &mut ctx).await);
    assert_eq!(
        Ok(format!("3")),
        parse_to_str("len [0, 1, 2]", &mut ctx).await
    );
}

#[tokio::test]
async fn test_map() {
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x} = {x: \"Hello, world\"} in x",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x} = {x: \"Hello, world\", y: 0} in x",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x} = {y: 0, x: \"Hello, world\"} in x",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\" a} = {y: 0, x: \"Hello, world\"} in a",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\": y} = {y: 0, x: \"Hello, world\"} in y",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("10".to_string()),
        parse_to_str(
            "if let {\"z\"} = {y: 0, x: \"Hello, world\"} then z else 10",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\": (@succ y)} = {y: 0, x: (@succ \"Hello, world\")} in y",
            &mut ContextHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn test_fn_right() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str("right ((x y) z) = right (x (y z))", &mut ctx)
        .await
        .is_ok());
    assert!(parse_to_str("right x = x", &mut ctx).await.is_ok());
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("right (@succ @succ @zero)", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ @zero)))".to_string()),
        parse_to_str("right (@succ @succ @succ @zero)", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ (@succ (@succ (@succ @zero))))))".to_string()),
        parse_to_str(
            "right (@succ @succ @succ @succ @succ @succ @zero)",
            &mut ctx
        )
        .await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ (@succ (@succ (@succ (@succ @zero)))))))".to_string()),
        parse_to_str(
            "right (@succ @succ @succ @succ @succ @succ @succ @zero)",
            &mut ctx
        )
        .await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ (@succ (@succ (@succ (@succ (@succ @zero))))))))".to_string()),
        parse_to_str(
            "right (@succ @succ @succ @succ @succ @succ @succ @succ @zero)",
            &mut ctx
        )
        .await
    );
}

#[tokio::test]
async fn test_fn_right_add() {
    let mut ctx = ContextHandler::async_default().await;
    assert!(parse_to_str("right ((x y) z) = right (x (y z))", &mut ctx)
        .await
        .is_ok());
    assert!(parse_to_str("right x = x", &mut ctx).await.is_ok());
    assert!(parse_to_str("add @zero y = y", &mut ctx).await.is_ok());
    assert!(parse_to_str("add (@succ x) y = add x (@succ y)", &mut ctx)
        .await
        .is_ok());
    assert_eq!(
        Ok("@zero".to_string()),
        parse_to_str("add @zero @zero", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ @zero)".to_string()),
        parse_to_str("add (@succ @zero) @zero", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ @zero)".to_string()),
        parse_to_str("add @zero (@succ @zero)", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("add (@succ (@succ @zero)) @zero", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("add @zero (@succ (@succ @zero))", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("add (@succ @zero) (@succ @zero)", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ @zero)))".to_string()),
        parse_to_str("add (@succ @zero) (@succ (@succ @zero))", &mut ctx).await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ @zero)))".to_string()),
        parse_to_str(
            "add (right (@succ @zero)) (right (@succ @succ @zero))",
            &mut ctx
        )
        .await
    );
}

#[tokio::test]
async fn test_minus_1() {
    assert_eq!(
        Ok(r"-1".to_string()),
        parse_to_str("-1", &mut ContextHandler::async_default().await).await
    );
}
