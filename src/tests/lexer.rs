use crate::lexer::Token;
use logos::Logos;
use num::FromPrimitive;

#[test]
fn test_str() {
    assert_eq!(
        Some(Token::Str(format!("test"))),
        Token::lexer("\"test\"").into_iter().next()
    );
}

#[test]
fn test_str_with_escape() {
    assert_eq!(
        Some(Token::Str(format!("\n"))),
        Token::lexer("\"\\n\"").into_iter().next()
    );

    assert_eq!(
        Some(Token::Str(format!("\\n"))),
        Token::lexer("\"\\\\n\"").into_iter().next()
    );
}

#[test]
fn test_int() {
    assert_eq!(
        Some(Token::Int(num::BigRational::from_integer(
            num::BigInt::from_u64(42).unwrap()
        ))),
        Token::lexer("42").into_iter().next()
    );
    assert_eq!(
        Some(Token::Int(num::BigRational::from_integer(
            num::BigInt::from_u64(142).unwrap()
        ))),
        Token::lexer("142").into_iter().next()
    );
}

#[test]
fn test_flt() {
    assert_eq!(
        Some(Token::Flt("42.5".to_string())),
        Token::lexer("42.5").into_iter().next()
    );
}

#[test]
fn test_id() {
    assert_eq!(
        Some(Token::Id(format!("test"))),
        Token::lexer("test").into_iter().next()
    );
    assert_eq!(
        Some(Token::Id(format!("x"))),
        Token::lexer("x").into_iter().next()
    );
}

#[test]
fn test_atom() {
    assert_eq!(
        Some(Token::Atom(format!("test"))),
        Token::lexer(":test").into_iter().next()
    );
    assert_eq!(
        Some(Token::Atom(format!("x"))),
        Token::lexer(":x").into_iter().next()
    );
}
