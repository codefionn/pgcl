use rowan::GreenNodeBuilder;

use crate::{
    errors::InterpreterError,
    execute::Syntax,
    lexer::Token,
    parser::{Parser, SyntaxElement, SyntaxKind},
};

fn parse(line: &str) -> Result<Syntax, InterpreterError> {
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
        ast.try_into()
    }
}

fn parse_to_str(line: &str) -> Result<String, InterpreterError> {
    let typed = parse(line)?;
    Ok(format!("{}", typed))
}

#[test]
fn parse_lambda() {
    assert_eq!(Ok(format!("(\\x x)")), parse_to_str("\\x x"));
    assert_eq!(Ok(format!("(\\x (x + y))")), parse_to_str("\\x x + y"));
}

#[test]
fn parse_let() {
    assert_eq!(
        Ok(format!("(let x = 0 in x)")),
        parse_to_str("let x = 0 in x")
    );
    assert_eq!(
        Ok(format!("(let x = 0 in (let y = 1 in x))")),
        parse_to_str("let x = 0; y = 1 in x")
    );
}
