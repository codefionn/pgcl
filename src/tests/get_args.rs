use rowan::GreenNodeBuilder;

use crate::{
    errors::InterpreterError,
    execute::Syntax,
    lexer::Token,
    parser::{Parser, SyntaxKind},
};

async fn get_args(line: &str) -> Result<Vec<String>, InterpreterError> {
    let toks = Token::lex_for_rowan(line)?;
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
        let mut result: Vec<String> = ast.get_args().await.into_iter().cloned().collect();
        result.sort_unstable();

        Ok(result)
    }
}

#[tokio::test]
async fn test_args() {
    assert_eq!(Ok(vec![]), get_args("2 + 3",).await);
    assert_eq!(Ok(vec!["x".to_string()]), get_args("x",).await);
    assert_eq!(
        Ok(["x", "y", "z"].into_iter().map(|s| s.to_string()).collect()),
        get_args("let x = y in z",).await
    );
    assert_eq!(
        Ok(["x", "y", "z"].into_iter().map(|s| s.to_string()).collect()),
        get_args("if let x = y then z else 0",).await
    );
    assert_eq!(
        Ok(["a", "x", "y", "z"]
            .into_iter()
            .map(|s| s.to_string())
            .collect()),
        get_args("if let x = y then z else a",).await
    );
    assert_eq!(
        Ok(["x", "xs"].into_iter().map(|s| s.to_string()).collect()),
        get_args("[x:xs]",).await
    );
    assert_eq!(
        Ok(["x", "y"].into_iter().map(|s| s.to_string()).collect()),
        get_args("[x, y]",).await
    );
    assert_eq!(
        Ok(["foldl", "x", "xs"]
            .into_iter()
            .map(|s| s.to_string())
            .collect()),
        get_args("(((foldl (,)) x) xs)",).await
    );
}
