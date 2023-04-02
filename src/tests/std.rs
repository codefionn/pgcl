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

    let (ast, errors) =
        Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse_main(true);
    if !errors.is_empty() {
        Err(InterpreterError::UnknownError())
    } else {
        let ast: Syntax = (*ast).try_into()?;
        ast.execute(true, ctx, system).await
    }
}

async fn parse_to_str(line: &str) -> Result<String, InterpreterError> {
    let mut ctx = ContextHandler::async_default().await;
    let mut system = SystemHandler::async_default().await;

    parse("std = import std", &mut ctx, &mut system).await?;
    let typed = parse(line, &mut ctx, &mut system).await?;
    Ok(format!("{}", typed))
}

#[tokio::test]
async fn test_let_map_from_std() {
    assert_eq!(
        Ok(format!("@atom")),
        parse_to_str("let { id } = std in id @atom").await
    );

    assert_eq!(
        Ok(format!("@atom")),
        parse_to_str("let { \"id\" as_id } = std in as_id @atom").await
    );
}

#[tokio::test]
async fn test_id() {
    assert_eq!(Ok(format!("5")), parse_to_str("std.id 5").await);
    assert_eq!(Ok(format!("@atom")), parse_to_str("std.id @atom").await);
}

#[tokio::test]
async fn test_right() {
    assert_eq!(
        Ok(format!("(@succ (@succ (@succ @zero)))")),
        parse_to_str("std.right (@succ @succ @succ @zero)").await
    );
    assert_eq!(
        Ok(format!("(1 (2 3))")),
        parse_to_str("std.right (1 2 3)").await
    );
}

#[tokio::test]
async fn test_map() {
    assert_eq!(
        Ok(format!("[1, 4, 9]")),
        parse_to_str(r"std.map (\x x * x) [1, 2, 3]").await
    );
}

#[tokio::test]
async fn test_foldl() {
    assert_eq!(
        Ok(format!("15")),
        parse_to_str(r"std.foldl (\x \y x + y) 0 [1, 2, 3, 4, 5]").await
    );

    assert_eq!(
        Ok(format!("-15")),
        parse_to_str(r"std.foldl (\x \y x - y) 0 [1, 2, 3, 4, 5]").await
    );

    assert_eq!(
        Ok(format!("-13")),
        parse_to_str(r"std.foldl (\x \y if x == 0 then y else x - y) 0 [1, 2, 3, 4, 5]").await
    );

    assert_eq!(
        Ok(format!("3")),
        parse_to_str(r"std.foldl (\x \y if x == 0 then y else y - x) 0 [1, 2, 3, 4, 5]").await
    );
}

#[tokio::test]
async fn test_foldr() {
    assert_eq!(
        Ok(format!("15")),
        parse_to_str(r"std.foldr (\x \y x + y) 0 [1, 2, 3, 4, 5]").await
    );

    assert_eq!(
        Ok(format!("-15")),
        parse_to_str(r"std.foldr (\x \y y - x) 0 [1, 2, 3, 4, 5]").await
    );

    assert_eq!(
        Ok(format!("-5")),
        parse_to_str(r"std.foldr (\x \y if y == 0 then x else y - x) 0 [1, 2, 3, 4, 5]").await
    );
}

#[tokio::test]
async fn test_head() {
    assert_eq!(Ok(format!("10")), parse_to_str(r"std.head [10]").await);
    assert_eq!(Ok(format!("10")), parse_to_str(r"std.head [10, 2]").await);
}

#[tokio::test]
async fn test_tail() {
    assert_eq!(Ok(format!("[3]")), parse_to_str(r"std.tail [2, 3]").await);
    assert_eq!(
        Ok(format!("[3, 1]")),
        parse_to_str(r"std.tail [2, 3, 1]").await
    );
    assert_eq!(
        Ok(format!("[3, 4, 1]")),
        parse_to_str(r"std.tail [2, 3, 4, 1]").await
    );
}
