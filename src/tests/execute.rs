use rowan::GreenNodeBuilder;

use crate::{
    context::{ContextHandler, ContextHolder},
    errors::{InterpreterError, LexerError},
    execute::{Executor, Syntax},
    lexer::Token,
    parser::{Parser, SyntaxKind},
    runner::Runner,
    system::SystemHandler,
    VerboseLevel,
};

async fn parse(
    line: &str,
    ctx: &mut ContextHandler,
    system: &mut SystemHandler,
) -> Result<Syntax, InterpreterError> {
    let toks = Token::lex_for_rowan(line)
        .map_err(|err| err.into())
        .map_err(|err: LexerError| err.into())?;
    let toks: Vec<(SyntaxKind, String)> = toks
        .into_iter()
        .map(|(tok, slice)| -> Result<(SyntaxKind, String), LexerError> {
            Ok((tok.clone().try_into()?, slice.clone()))
        })
        .try_collect()
        .map_err(|err: LexerError| err.into())?;

    let (ast, errors) = Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
    if !errors.is_empty() {
        Err(InterpreterError::UnknownError())
    } else {
        let ast: Syntax = (*ast).try_into()?;
        let mut runner = Runner::new(system)
            .await
            .map_err(|err| InterpreterError::InternalError(format!("{}", err)))?;
        Executor::new(ctx, system, &mut runner, VerboseLevel::None, true)
            .execute(ast, true)
            .await
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("11")),
        parse_to_str(
            "2 + 3 * 3",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("5")),
        parse_to_str(
            "5 # Test",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(r"3".to_string()),
        parse_to_str(
            "1\n| \\x x + 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(r"3".to_string()),
        parse_to_str(
            "1\n\n| \\x x + 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("7")),
        parse_to_str(
            "2.5 + 4.5",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("7")),
        parse_to_str(
            "let x = 2.5 in x + 4.5",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("6")),
        parse_to_str(
            "1 * 6",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(
            "3 == 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(
            "_ == 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("@true")),
        parse_to_str(
            "3 != 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(
            "_ != 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("@false")),
        parse_to_str(
            "2 != _",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = \y = x + y) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok(format!("12")),
        parse_to_str(
            r"(\x = (\y x + y)) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn fn_add_fn_and_lambda() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
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
    let mut system = SystemHandler::default();
    assert!(parse_to_str("len [] = 0", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(
        parse_to_str("len [x:xs] = 1 + len xs", &mut ctx, &mut system)
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
    let mut system = SystemHandler::default();
    assert!(parse_to_str("len [] = 0", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(parse_to_str("len [x] = 1 ", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(parse_to_str("len [x:xs] = 2", &mut ctx, &mut system)
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let { x } = { x: \"Hello, world\", y: 0 } in x",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let { x } = { y: 0, x: \"Hello, world\" } in x",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let { \"x\" a } = { y: 0, x: \"Hello, world\" } in a",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\": y} = {y: 0, x: \"Hello, world\"} in y",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {x: y} = {y: 0, x: \"Hello, world\"} in y",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("10".to_string()),
        parse_to_str(
            "if let {\"z\"} = {y: 0, x: \"Hello, world\"} then z else 10",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"Hello, world\"".to_string()),
        parse_to_str(
            "let {\"x\": (@succ y)} = {y: 0, x: (@succ \"Hello, world\")} in y",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("{}".to_string()),
        parse_to_str(
            "{}",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("0".to_string()),
        parse_to_str(
            "{ x: 0, y: 1}.x",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("1".to_string()),
        parse_to_str(
            "{ x: 0, y: 1}.y",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_fn_right() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

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
    let mut system = SystemHandler::default();

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
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_two_ctx() {
    let mut holder = ContextHolder::default();
    let mut system = SystemHandler::default();
    let mut ctx0 = holder
        .create_context("0".to_string(), None)
        .await
        .handler(holder.clone());
    let mut ctx1 = holder
        .create_context("1".to_string(), None)
        .await
        .handler(holder.clone());
    let ctx0_id = ctx0.get_id();
    let system_id = system.get_id();

    assert!(parse_to_str("x = 1", &mut ctx0, &mut system).await.is_ok());
    let mut runner = Runner::new(&mut system).await.unwrap();
    assert_eq!(
        Ok(r"1".to_string()),
        Executor::new(
            &mut ctx1,
            &mut system,
            &mut runner,
            VerboseLevel::None,
            true
        )
        .execute(
            Syntax::Contextual(ctx0_id, system_id, Box::new(Syntax::Id("x".to_string()))),
            true
        )
        .await
        .map(|expr| format!("{}", expr))
    );
}

#[tokio::test]
async fn test_export() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

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
    let mut system = SystemHandler::default();

    assert!(parse_to_str("std = import std", &mut ctx, &mut system)
        .await
        .is_ok());
    assert_eq!(
        Ok("21".to_string()),
        parse_to_str("std.id 21", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok("(import \"std\")".to_string()),
        parse_to_str("import std", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_map_add() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

    assert_eq!(
        Ok("{ x: 10, y: 12 }".to_string()),
        parse_to_str("{ x: 10 } + { y: 12 }", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok("{ x: 10, y: 12 }".to_string()),
        parse_to_str("{ x: 10 } + { x: 13, y: 12 }", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok("{ x: 10, y: 12, z: 2 }".to_string()),
        parse_to_str(
            "{ x: 10 } + { x: 13, y: 12 } + {z: 2}",
            &mut ctx,
            &mut system
        )
        .await
    );

    assert_eq!(
        Ok("{ x: 10, y: 12, z: 2 }".to_string()),
        parse_to_str(
            "{ x: 10 } + ({ x: 13, y: 12 } + {z: 2})",
            &mut ctx,
            &mut system
        )
        .await
    );
}

#[tokio::test]
async fn test_int_div() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

    assert_eq!(
        Ok("2.5".to_string()),
        parse_to_str("5 / 2", &mut ctx, &mut system).await
    );

    assert_eq!(
        Ok("4000".to_string()),
        parse_to_str("48000 / 12", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_float() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(10, 2)".to_string()),
        parse_to_str(
            r"(\x \y (x, y)) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            r"(\x \y x == y) 10 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            r"(\x \y x == y) 10 (8 + 2)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            r"(\x let (1 x) = (1 x) in x) 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            r"(\x \y let (1 y) = (1 y) in x) 2 10",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("10".to_string()),
        parse_to_str(
            r"(\x \y let (1 y) = (1 y) in y) 2 10",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
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
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@float".to_string()),
        parse_to_str(
            r"syscall (@type, 2.0)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@float".to_string()),
        parse_to_str(
            r"syscall (@type, 2.0 + 1.0)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@string".to_string()),
        parse_to_str(
            "syscall (@type, \"\")",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@list".to_string()),
        parse_to_str(
            r"syscall (@type, [])",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@map".to_string()),
        parse_to_str(
            r"syscall (@type, {})",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@lambda".to_string()),
        parse_to_str(
            r"syscall (@type, (\x x))",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@tuple (@int, @int))".to_string()),
        parse_to_str(
            r"syscall (@type, (2, 3))",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@tuple (@int, @list))".to_string()),
        parse_to_str(
            r"syscall (@type, (2, []))",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_syscall_cmd() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

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

#[tokio::test]
async fn test_lst_str() {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

    assert!(parse_to_str("len [] = 0", &mut ctx, &mut system)
        .await
        .is_ok());
    assert!(
        parse_to_str("len [x:xs] = 1 + len xs", &mut ctx, &mut system)
            .await
            .is_ok()
    );
    assert_eq!(
        Ok("0".to_string()),
        parse_to_str("len \"\"", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("1".to_string()),
        parse_to_str("len \"t\"", &mut ctx, &mut system).await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str("len \"test\"", &mut ctx, &mut system).await
    );
}

#[tokio::test]
async fn test_str_starts_with() {
    assert_eq!(
        Ok("\"est\"".to_string()),
        parse_to_str(
            "if let [\"t\":x] = \"test\" then x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("0".to_string()),
        parse_to_str(
            "if let [\"x\":x] = \"test\" then x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"st\"".to_string()),
        parse_to_str(
            "if let [\"te\":x] = \"test\" then x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("0".to_string()),
        parse_to_str(
            "if let [\"tx\":x] = \"test\" then x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"t\"".to_string()),
        parse_to_str(
            "if let [\"tes\":x] = \"test\" then x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"\"".to_string()),
        parse_to_str(
            "if let [\"test\":x] = \"test\" then x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("0".to_string()),
        parse_to_str(
            "if let [\"testx\":x] = \"test\" then x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(\"s\" \"\")".to_string()),
        parse_to_str(
            "if let [\"te\":y:\"t\":x] = \"test\" then y x else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"tt\"".to_string()),
        parse_to_str(
            "if let [x:\"es\":y] = \"test\" then x + y else 0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_js_bool() {
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            "false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "true",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("{ y: @false }".to_string()),
        parse_to_str(
            "{ y: false }",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("{ x: @true }".to_string()),
        parse_to_str(
            "{ x: true }",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_pipe() {
    assert_eq!(
        Ok("3".to_string()),
        parse_to_str(
            r"1 | (\x \y x + y) 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            r"2 | (\x \y x / y) 4",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_if_let() {
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            r"if let 0.1 = 0.1 then @true else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            r"if let 0.2 = 0.1 then @true else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "if let \"x\" = \"x\" then @true else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            "if let \"x\" = \"y\" then @true else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn equal_type_coersion() {
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            "2.0 == 1.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "2.0 == 2.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            "2.0 == 1",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "2.0 == 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            "2 == 1.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "2 == 2.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "_ == 2.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "2 == _",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn pow() {
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "2 ** 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "2.0 ** 2",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "2 ** 2.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("16".to_string()),
        parse_to_str(
            "4 ** 2.0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("8".to_string()),
        parse_to_str(
            "4 ** (3 / 2)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_fn_op() {
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "(+) 3 1",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_match() {
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match @true then @true => 4, @false => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            "match @false then @true => 4, @false => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match @true then @true => 4, _ => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            "match @false then @true => 4, _ => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("2".to_string()),
        parse_to_str(
            "match @false then @true => 4, @false => 2, _ => 1;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match 2 + 2 then 4 => 4, _ => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match 2 + 2 then x => x, _ => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match 2 + 2 then x => x;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match\n 2 + 2 \nthen\n x =>\n x\n,\n _ => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match\n 2 + 2 \nthen\n x\n =>\n x\n,\n _ => 2;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("4".to_string()),
        parse_to_str(
            "match 2 + 2\nthen\n x => x\n;",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_regex() {
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "r/test/ \"test\"",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "r/^test$/ \"test\"",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\"])".to_string()),
        parse_to_str(
            "let (r/^test$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\", \"te\"])".to_string()),
        parse_to_str(
            "let (r/^(te)st$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\", \"te\"])".to_string()),
        parse_to_str(
            "let (r/^(te)?st$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"st\", @none])".to_string()),
        parse_to_str(
            "let (r/^(te)?st$/ m) = \"st\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\", \"te\", \"e\"])".to_string()),
        parse_to_str(
            "let (r/^(t(e))?st$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_regex_no_r() {
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "/test/ \"test\"",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "/^test$/ \"test\"",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\"])".to_string()),
        parse_to_str(
            "let (/^test$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\", \"te\"])".to_string()),
        parse_to_str(
            "let (/^(te)st$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\", \"te\"])".to_string()),
        parse_to_str(
            "let (/^(te)?st$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"st\", @none])".to_string()),
        parse_to_str(
            "let (/^(te)?st$/ m) = \"st\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(@true, [\"test\", \"te\", \"e\"])".to_string()),
        parse_to_str(
            "let (/^(t(e))?st$/ m) = \"test\" in m",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_map_add_lst() {
    assert_eq!(
        Ok("{}".to_string()),
        parse_to_str(
            "{} + []",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("{ test: 0 }".to_string()),
        parse_to_str(
            "{test: 0} + []",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("{ test0: 0, test1: 1 }".to_string()),
        parse_to_str(
            "{test0: 0} + [\"test1\", 1]",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("{ test0: 0, test1: 1, test2: 2 }".to_string()),
        parse_to_str(
            "{test0: 0} + [\"test1\", 1, \"test2\", 2]",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("{ test0: 3, test1: 1, test2: 2 }".to_string()),
        parse_to_str(
            "{test0: 0} + [\"test1\", 1, \"test2\", 2, \"test0\", 3]",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("{ test: 0 }".to_string()),
        parse_to_str(
            "{} + [\"test\", 0]",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("{ \"0\": 0 }".to_string()),
        parse_to_str(
            "{} + [0, 0]",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("{ \"test-id\": 0 }".to_string()),
        parse_to_str(
            "{} + [\"test-id\", 0]",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_empty_tuple() {
    assert_eq!(
        Ok("()".to_string()),
        parse_to_str(
            "()",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("()".to_string()),
        parse_to_str(
            "(())",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str(
            "() == ()",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_infinite_match_list() {
    assert_eq!(
        Ok("0".to_string()),
        parse_to_str(
            "nat x = [x] + (nat (x + 1))\nlet [x:xs] = nat 0 in x",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );

    assert_eq!(
        Ok("(0, 1)".to_string()),
        parse_to_str(
            "nat x = [x] + (nat (x + 1))\nlet [x:y:xs] = nat 0 in (x, y)",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_infinite_match_map() {
    assert_eq!(
        Ok("0".to_string()),
        parse_to_str(
            "nat x = (nat (x + 1)) + {} + [\"key\" + x, x]\nlet { key0 } = nat 0 in key0",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("1".to_string()),
        parse_to_str(
            "nat x = (nat (x + 1)) + {} + [\"key\" + x, x]\nlet { key1 } = nat 0 in key1",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("100".to_string()),
        parse_to_str(
            "nat x = (nat (x + 1)) + {} + [\"key\" + x, x]\nlet { key100 } = nat 0 in key100",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}

#[tokio::test]
async fn test_contains_string() {
    assert_eq!(
        Ok("\"xy\"".to_string()),
        parse_to_str(
            "if let [x:\"test\":y] = \"xtesty\" then x + y else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"x\"".to_string()),
        parse_to_str(
            "if let [x:\"test\":y] = \"xtest\" then x + y else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("\"x01y\"".to_string()),
        parse_to_str(
            "if let [x:\"test\":y] = \"x0test1y\" then x + y else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("(\"x0\", \"1y\")".to_string()),
        parse_to_str(
            "if let [x:\"test\":y] = \"x0test1y\" then (x, y) else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str(
            "if let [x:\"test\":y] = \"x0tes1y\" then x + y else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
    assert_eq!(
        Ok("((\"x\", \"0\"), \"1y\")".to_string()),
        parse_to_str(
            "if let [z:x:\"test\":y] = \"x0test1y\" then (z, x, y) else @false",
            &mut ContextHandler::async_default().await,
            &mut SystemHandler::default()
        )
        .await
    );
}
