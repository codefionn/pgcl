use rowan::GreenNodeBuilder;

use crate::{
    context::ContextHandler,
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
    let toks = Token::lex_for_rowan(line).map_err(|err| err.into())?;
    let toks: Vec<(SyntaxKind, String)> = toks
        .into_iter()
        .map(|(tok, slice)| -> Result<(SyntaxKind, String), LexerError> {
            Ok((tok.clone().try_into()?, slice.clone()))
        })
        .try_collect()
        .map_err(|err| err.into())?;

    let (ast, errors) =
        Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse_main(true);
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

async fn parse_to_str(line: &str) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

    parse("sys = import sys", &mut ctx, &mut system).await?;
    let typed = parse(line, &mut ctx, &mut system).await?;
    Ok(format!("{}", typed))
}

async fn parse_to_str_custom(line: &str) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::default();

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
    let mut system = SystemHandler::default();

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
    let mut system = SystemHandler::default();

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
    let mut system = SystemHandler::default();

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
        Ok("(@error @RestrictedSystemCall)".to_string()),
        parse_to_str_restrict(r"sys.type 2.0").await
    );
}

#[tokio::test]
async fn test_typeof_intentionally_broken_test_override() {
    assert_eq!(
        Ok("(@error @RestrictedSystemCall)".to_string()),
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

#[tokio::test]
async fn test_json() {
    assert_eq!(
        Ok("{ test: 0 }".to_string()),
        parse_to_str("sys.json.decode (sys.json.encode { test: 0 })").await
    );
}

#[tokio::test]
async fn test_fs() {
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str("sys.fs.write \"test\" \"1\"").await
    );
    assert_eq!(
        Ok("\"1\"".to_string()),
        parse_to_str("sys.fs.read \"test\"").await
    );
    assert_eq!(
        Ok("@true".to_string()),
        parse_to_str("sys.fs.delete \"test\"").await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("if let (@error _) = sys.fs.read \"test\" then @false else @true").await
    );
    assert_eq!(
        Ok("@false".to_string()),
        parse_to_str("if let (@error _) = sys.fs.delete \"test\" then @false else @true").await
    );
}
