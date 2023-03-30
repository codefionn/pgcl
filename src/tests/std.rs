use rowan::GreenNodeBuilder;

use crate::{
    context::{ContextHandler, ContextHolder},
    errors::InterpreterError,
    execute::Syntax,
    lexer::Token,
    parser::{Parser, SyntaxKind},
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

    let (ast, errors) =
        Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse_main(true);
    if !errors.is_empty() {
        Err(InterpreterError::UnknownError())
    } else {
        let ast: Syntax = (*ast).try_into()?;
        ast.execute(true, ctx).await
    }
}

async fn parse_to_str(line: &str, ctx: &mut ContextHandler) -> Result<String, InterpreterError> {
    parse("std = import std", ctx).await?;
    let typed = parse(line, ctx).await?;
    Ok(format!("{}", typed))
}

#[tokio::test]
async fn test_id() {
    assert_eq!(
        Ok(format!("5")),
        parse_to_str("std.id 5", &mut ContextHandler::async_default().await).await
    );
    assert_eq!(
        Ok(format!("@atom")),
        parse_to_str("std.id @atom", &mut ContextHandler::async_default().await).await
    );
}

#[tokio::test]
async fn test_right() {
    assert_eq!(
        Ok(format!("(@succ (@succ (@succ @zero)))")),
        parse_to_str(
            "std.right (@succ @succ @succ @zero)",
            &mut ContextHandler::async_default().await
        )
        .await
    );
    assert_eq!(
        Ok(format!("(1 (2 3))")),
        parse_to_str(
            "std.right (1 2 3)",
            &mut ContextHandler::async_default().await
        )
        .await
    );
}

#[tokio::test]
async fn test_map() {
    assert_eq!(
        Ok(format!("[1, 4, 9]")),
        parse_to_str(
            r"std.map (\x x * x) [1, 2, 3]",
            &mut ContextHandler::async_default().await
        )
        .await
    );
}
