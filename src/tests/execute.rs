use rowan::GreenNodeBuilder;

use crate::{
    context::{ContextHandler, ContextHolder},
    errors::InterpreterError,
    execute::Syntax,
    lexer::Token,
    parser::{Parser, SyntaxKind},
    system::SystemHandler,
};

async fn parse(
    line: &str,
    ctx: &mut ContextHandler,
    system: &mut SystemHandler,
) -> Result<Syntax, InterpreterError> {
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
        ast.execute(true, ctx, system).await
    }
}

async fn parse_to_str(
    line: &str,
    ctx: &mut ContextHandler,
    system: &mut SystemHandler,
) -> Result<String, InterpreterError> {
    let typed = parse(line, ctx, system).await?;
    Ok(format!("{}", typed))
}

#[tokio::test]
async fn add_integers() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str(
            "2 + 3",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("11")),
        parse_to_str(
            "2 + 3 * 3",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn test_comments() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str(
            "5 // Test",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("5")),
        parse_to_str(
            "5 # Test",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn add_with_pipe() {
    assert_eq!(
        Ok(r"3".to_string()),
        parse_to_str(
            r"1 | \x x + 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn add_floats() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str(
            "2.0 + 3.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("7")),
        parse_to_str(
            "2.5 + 4.5",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn more_maths() {
    assert_eq!(
        Ok(format!("1")),
        parse_to_str(
            "2.0 / 2.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("6")),
        parse_to_str(
            "1 * 6",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn eq() {
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(
            "2 == 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(
            "3 == 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(
            "_ == 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn add_str() {
    assert_eq!(
        Ok(format!("\"helloworld\"")),
        parse_to_str(
            "\"hello\" + \"world\"",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn neq() {
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(
            "2 != 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(
            "3 != 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(
            "_ != 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(
            "2 != _",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn fn_add_lambda() {
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = \y = x + y) 2 10",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = \y = x + y) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = (\y x + y)) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn fn_add_fn_and_lambda() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str(r"add x = \y = x + y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add 10 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add 2 10", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn fn_add_fn() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add 10 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add 2 10", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn fn_add_fn_tuple() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str(r"add (x, y) = x + y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add (10, 2)", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(r"add (2, 10)", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_fn_atom() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str("add @zero y = y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(
        parse_to_str("add (@succ x) y = add x (@succ y)", &mut ctx, &mut system)
            .await
            .is_ok()
    );
    assert_eq!(
        Ok(format!(r"@zero")),
        parse_to_str(r"add @zero @zero", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!(r"(@succ @zero)")),
        parse_to_str(r"add @zero (@succ @zero)", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!(r"(@succ (@succ @zero))")),
        parse_to_str(r"add @zero (@succ (@succ @zero))", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!(r"(@succ (@succ (@succ @zero)))")),
        parse_to_str(
            r"add (@succ @zero) (@succ (@succ @zero))",
            &mut ctx,
            &mut system
        )
        .await
    );
    assert_eq!(
        Ok(format!(r"(@succ (@succ (@succ (@succ (@succ @zero)))))")),
        parse_to_str(
            r"add (@succ (@succ (@succ @zero))) (@succ (@succ @zero))",
            &mut ctx,
            &mut system
        )
        .await
    );
}

#[tokio::test]
async fn op_geq() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 >= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 >= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 2.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 3.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 0.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 1.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 >= 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 >= 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 >= 5.0", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 >= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1.0 >= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2.0 >= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 4) >= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 3) >= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 2) >= 5", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn op_gt() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 > 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 > 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 > 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 2.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 3.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 0.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 > 1.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 > 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1 > 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2 > 5.0", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 > 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 > 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"1.0 > 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"2.0 > 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 4) > 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 3) > 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 2) > 5", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn op_leq() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 <= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1 <= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2 <= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 2.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 3.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 0.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 <= 1.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5 <= 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1 <= 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2 <= 5.0", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 <= 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"5.0 <= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1.0 <= 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2.0 <= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 4) <= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 3) <= 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 2) <= 5", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn op_lt() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str(r"add x y = x + y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1 < 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2 < 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 2.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 3.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 0.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 1.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5 < 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1 < 5.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2 < 5.0", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"5.0 < 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"1.0 < 5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"2.0 < 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 4) < 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(r"(add 2 3) < 5", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(r"(add 2 2) < 5", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn recursion_fib() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str("fib 1 = 1", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(parse_to_str("fib 2 = 1", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(
        parse_to_str("fib x = fib (x-2) + fib (x-1)", &mut ctx, &mut system)
            .await
            .is_ok()
    );
    assert_eq!(
        Ok(format!("55")),
        parse_to_str("fib 10", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn pattern_match_list() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str("len [] = 0", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(
        parse_to_str("len [x;xs] = 1 + len xs", &mut ctx, &mut system)
            .await
            .is_ok()
    );
    assert_eq!(
        Ok(format!("0")),
        parse_to_str("len []", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("1")),
        parse_to_str("len [0]", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("2")),
        parse_to_str("len [0, 1]", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("3")),
        parse_to_str("len [0, 1, 2]", &mut ctx, &mut system).await
    );

    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;
    assert!(parse_to_str("len [] = 0", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(parse_to_str("len [x] = 1 ", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(parse_to_str("len [x;xs] = 2", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok(format!("0")),
        parse_to_str("len []", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("1")),
        parse_to_str("len [1]", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("1")),
        parse_to_str("len [2]", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("2")),
        parse_to_str("len [1, 2]", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok(format!("2")),
        parse_to_str("len [1, 2, 3]", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_map() {
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let { x } = { x: \"Hello, world\" } in x",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let { x } = { x: \"Hello, world\", y: 0 } in x",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let { x } = { y: 0, x: \"Hello, world\" } in x",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let { \"x\" a } = { y: 0, x: \"Hello, world\" } in a",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\": y} = {y: 0, x: \"Hello, world\"} in y",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x: y} = {y: 0, x: \"Hello, world\"} in y",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("10".to_string()),
        parse_to_str(
            "if let {\"z\"} = {y: 0, x: \"Hello, world\"} then z else 10",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\": (@succ y)} = {y: 0, x: (@succ \"Hello, world\")} in y",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("{}".to_string()),
        parse_to_str(
            "{}",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn test_fn_right() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    assert!(
        parse_to_str("right ((x y) z) = right (x (y z))", &mut ctx, &mut system)
            .await
            .is_ok()
    );
    assert!(parse_to_str("right x = x", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("right (@succ @succ @zero)", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ @zero)))".to_string()),
        parse_to_str("right (@succ @succ @succ @zero)", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ (@succ (@succ (@succ @zero))))))".to_string()),
        parse_to_str(
            "right (@succ @succ @succ @succ @succ @succ @zero)",
            &mut ctx,
            &mut system
        )
        .await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ (@succ (@succ (@succ (@succ @zero)))))))".to_string()),
        parse_to_str(
            "right (@succ @succ @succ @succ @succ @succ @succ @zero)",
            &mut ctx,
            &mut system
        )
        .await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ (@succ (@succ (@succ (@succ (@succ @zero))))))))".to_string()),
        parse_to_str(
            "right (@succ @succ @succ @succ @succ @succ @succ @succ @zero)",
            &mut ctx,
            &mut system
        )
        .await
    );
}

#[tokio::test]
async fn test_fn_right_add() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    assert!(
        parse_to_str("right ((x y) z) = right (x (y z))", &mut ctx, &mut system)
            .await
            .is_ok()
    );
    assert!(parse_to_str("right x = x", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(parse_to_str("add @zero y = y", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(
        parse_to_str("add (@succ x) y = add x (@succ y)", &mut ctx, &mut system)
            .await
            .is_ok()
    );
    assert_eq!(
        Ok("@zero".to_string()),
        parse_to_str("add @zero @zero", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ @zero)".to_string()),
        parse_to_str("add (@succ @zero) @zero", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ @zero)".to_string()),
        parse_to_str("add @zero (@succ @zero)", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("add (@succ (@succ @zero)) @zero", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("add @zero (@succ (@succ @zero))", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ (@succ @zero))".to_string()),
        parse_to_str("add (@succ @zero) (@succ @zero)", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ @zero)))".to_string()),
        parse_to_str(
            "add (@succ @zero) (@succ (@succ @zero))",
            &mut ctx,
            &mut system
        )
        .await
    );
    assert_eq!(
        Ok("(@succ (@succ (@succ @zero)))".to_string()),
        parse_to_str(
            "add (right (@succ @zero)) (right (@succ @succ @zero))",
            &mut ctx,
            &mut system
        )
        .await
    );
}

#[tokio::test]
async fn test_minus_1() {
    assert_eq!(
        Ok(r"-1".to_string()),
        parse_to_str(
            "-1",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn test_two_ctx() {
    let mut holder = ContextHolder::default();
    let mut system = SystemHandler::async_default().await;
    let mut ctx0 = holder
        .create_context("0".to_string(), None)
        .await
        .handler(holder.clone());
    let mut ctx1 = holder
        .create_context("1".to_string(), None)
        .await
        .handler(holder.clone());

    assert!(parse_to_str("x = 1", &mut ctx0, &mut system).await.is_ok());
    assert_eq!(
        Ok(r"1".to_string()),
        Syntax::Contextual(
            ctx0.get_id(),
            system.get_id(),
            Box::new(Syntax::Id("x".to_string()))
        )
        .execute(true, &mut ctx1, &mut system)
        .await
        .map(|expr| format!("{}", expr))
    );
}

#[tokio::test]
async fn test_export() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    assert!(parse_to_str("id x = x", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(parse_to_str("export id", &mut ctx, &mut system)
        .await
        .is_ok());

    assert_eq!(Vec::<String>::new(), ctx.get_errors().await);
    assert!(ctx.get_global(&"id".to_string()).await.is_some());
}

#[tokio::test]
async fn test_import_std() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    assert!(parse_to_str("std = import std", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok("21".to_string()),
        parse_to_str("std.id 21", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok("std".to_string()),
        parse_to_str("import std", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_map_add() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    assert_eq!(
        Ok("{ x: 10, y: 12 }".to_string()),
        parse_to_str("{ x: 10 } + { y: 12 }", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok("{ x: 10, y: 12 }".to_string()),
        parse_to_str("{ x: 10 } + { x: 13, y: 12 }", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_int_div() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    assert_eq!(
        Ok("2.5".to_string()),
        parse_to_str("5 / 2", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_float() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    assert_eq!(
        Ok("1".to_string()),
        parse_to_str("1.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("1.2".to_string()),
        parse_to_str("1.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str("1.0 + 1.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str("1.0 + 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str("1 + 1.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str("0.5 + 1.5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("-0.5".to_string()),
        parse_to_str("1 - 1.5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("-0.5".to_string()),
        parse_to_str("0.5 - 1", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("-1".to_string()),
        parse_to_str("0.5 - 1.5", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("6.6".to_string()),
        parse_to_str("2.2 * 3.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("6.6".to_string()),
        parse_to_str("2.2 * 3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("6.6".to_string()),
        parse_to_str("3 * 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("3".to_string()),
        parse_to_str("6.6 / 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("3".to_string()),
        parse_to_str("6.0 / 2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("3".to_string()),
        parse_to_str("6 / 2.0", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str("6.6 >= 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("2.2 > 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("2.2 > 2.3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("2.2 > 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str("2.2 >= 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("2.2 >= 2.3", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str("1.2 < 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("2.2 < 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("3.2 < 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str("1.2 <= 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("3.2 <= 2.2", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str("2.2 <= 2.2", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_lambda() {
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            r"(\x \x x) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("(10, 2)".to_string()),
        parse_to_str(
            r"(\x \y (x, y)) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            r"(\x \y x == y) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            r"(\x \y x == y) 10 (8 + 2)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            r"(\x let (1 x) = (1 x) in x) 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            r"(\x \y let (1 y) = (1 y) in x) 2 10",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("10".to_string()),
        parse_to_str(
            r"(\x \y let (1 y) = (1 y) in y) 2 10",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn test_syscall_type() {
    assert_eq!(
        Ok("@int".to_string()),
        parse_to_str(
            r"syscall (@type, 2)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@float".to_string()),
        parse_to_str(
            r"syscall (@type, 2.0)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@float".to_string()),
        parse_to_str(
            r"syscall (@type, 2.0 + 1.0)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@string".to_string()),
        parse_to_str(
            "syscall (@type, \"\")",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@list".to_string()),
        parse_to_str(
            r"syscall (@type, [])",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@map".to_string()),
        parse_to_str(
            r"syscall (@type, {})",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("@lambda".to_string()),
        parse_to_str(
            r"syscall (@type, (\x x))",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("(@tuple (@int, @int))".to_string()),
        parse_to_str(
            r"syscall (@type, (2, 3))",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok("(@tuple (@int, @list))".to_string()),
        parse_to_str(
            r"syscall (@type, (2, []))",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn test_syscall_cmd() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    let result = if cfg!(target_os = "windows") {
        parse_to_str("syscall (@cmd, \"dir\")", &mut ctx, &mut system).await
    } else {
        parse_to_str("syscall (@cmd, \"ls\")", &mut ctx, &mut system).await
    };

    assert!(result.is_ok());
    let result = result.unwrap();
    let test = regex::Regex::new(
        r"\{ ((status: 0|stdout: .*|stderr: .*), ){2}(status: 0|stdout: .*|stderr: .*) \}",
    )
    .unwrap();
    println!("{}", result);
    assert!(test.is_match(result.as_str()));
}
