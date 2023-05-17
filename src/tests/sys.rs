use rowan::GreenNodeBuilder;

use crate::{
    context::{ContextHandler, ContextHolder},
    errors::InterpreterError,
    execute::{Syntax, Executor},
    lexer::Token,
    parser::{Parser, SyntaxKind},
    system::SystemHandler
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

    let (ast, errors) =
        Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse_main(true);
    if !errors.is_empty() {
        Err(InterpreterError::UnknownError())
    } else {
        let ast: Syntax = (*ast).try_into()?;
        Executor::new(ctx, system).execute(ast, true).await
    }
}

async fn parse_to_str(line: &str) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    parse("sys = import sys", &mut ctx, &mut system).await?;
    let typed = parse(line, &mut ctx, &mut system).await?;
    Ok(format!("{}", typed))
}

async fn parse_to_str_custom(line: &str) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    parse(
        "sys = import (sys, { type: @error })",
        &mut ctx,
        &mut system,
    )
    .await?;
    let typed = parse(line, &mut ctx, &mut system).await?;
    Ok(format!("{}", typed))
}

async fn parse_to_str_restrict(line: &str) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    parse(
        "sys = import (sys, { restrict: @true })",
        &mut ctx,
        &mut system,
    )
    .await?;
    let typed = parse(line, &mut ctx, &mut system).await?;
    Ok(format!("{}", typed))
}

async fn parse_to_str_restrict_and_restrict_insecure(
    line: &str,
) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    parse(
        "sys = import (sys, { restrict_insecure: @true, restrict: @true })",
        &mut ctx,
        &mut system,
    )
    .await?;
    let typed = parse(line, &mut ctx, &mut system).await?;
    Ok(format!("{}", typed))
}

async fn parse_to_str_restrict_insecure(line: &str) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    parse(
        "sys = import (sys, { restrict_insecure: @true })",
        &mut ctx,
        &mut system,
    )
    .await?;
    let typed = parse(line, &mut ctx, &mut system).await?;
    Ok(format!("{}", typed))
}

#[tokio::test]
async fn test_typeof() {
    assert_eq!(
        Ok("@float".to_string()),
        parse_to_str(r"sys.type 2.0").await
    );
}

#[tokio::test]
async fn test_typeof_intentionally_broken() {
    assert_eq!(
        Ok("@error".to_string()),
        parse_to_str_custom(r"sys.type 2.0").await
    );
    assert_eq!(
        Ok("@error".to_string()),
        parse_to_str_restrict(r"sys.type 2.0").await
    );
}

#[tokio::test]
async fn test_typeof_intentionally_broken_test_override() {
    assert_eq!(
        Ok("@error".to_string()),
        parse_to_str_restrict_and_restrict_insecure(r"sys.type 2.0").await
    );
}

#[tokio::test]
async fn test_typeof_restrict_insecure() {
    assert_eq!(
        Ok("@float".to_string()),
        parse_to_str_restrict_insecure(r"sys.type 2.0").await
    );
}

#[tokio::test]
async fn test_msg_channel() {
    assert_eq!(
        Ok("42".to_string()),
        parse_to_str("msg = sys.channel\nmsg\nmsg 42\nsys.recv_channel msg").await
    );
}
