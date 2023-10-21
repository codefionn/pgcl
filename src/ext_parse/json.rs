/*
 * Implements RFC 8259
 */

use std::{collections::BTreeMap, iter::Peekable, str::CharIndices};

use bigdecimal::{num_bigint::Sign, BigDecimal, FromPrimitive};
use num::BigInt;

use crate::{execute::Syntax, lexer::Token, rational::BigRational};

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum EncodeError {
    NotImplemented,
    ReachedDepthLimit,
    UnexpectedExpr(Syntax),
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct EncodeOptions {
    pub depth: Option<usize>,
    pub pretty: bool,
    pub indent: String,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            depth: None,
            pretty: false,
            indent: "  ".to_string(),
        }
    }
}

pub fn encode_with_options(
    expr: Syntax,
    encode_options: &EncodeOptions,
) -> Result<String, EncodeError> {
    encode_rec(expr, encode_options, 0)
}

pub fn encode(expr: Syntax) -> Result<String, EncodeError> {
    encode_with_options(expr, &Default::default())
}

pub fn encode_pretty(expr: Syntax) -> Result<String, EncodeError> {
    encode_with_options(
        expr,
        &EncodeOptions {
            pretty: true,
            ..Default::default()
        },
    )
}

fn escape_str(s: String) -> String {
    s.chars()
        .map(|c| match c {
            '\\' => r"\\".to_string(),
            '"' => "\\\"".to_string(),
            '\u{0008}' => r"\b".to_string(),
            '\u{000C}' => r"\f".to_string(),
            '\n' => r"\n".to_string(),
            '\r' => r"\r".to_string(),
            '\t' => r"\t".to_string(),
            c if (0x0000..=0x001F).contains(&(c as u32)) => format!("u00{:x}", c as u32),
            c => format!("{}", c),
        })
        .collect()
}

pub fn encode_rec(
    expr: Syntax,
    encode_options: &EncodeOptions,
    depth: usize,
) -> Result<String, EncodeError> {
    if let Some(max_depth) = encode_options.depth {
        if depth > max_depth {
            return Err(EncodeError::ReachedDepthLimit);
        }
    }

    fn create_indent(encode_options: &EncodeOptions, depth: usize) -> String {
        encode_options.indent.repeat(depth)
    }

    match expr {
        Syntax::Map(map) if map.is_empty() => Ok("{}".to_string()),
        Syntax::Map(map) => {
            let mut result = String::with_capacity(
                128 * map.len()
                    + if encode_options.pretty {
                        encode_options.indent.len() * map.len()
                    } else {
                        0
                    },
            );
            result += "{";
            if encode_options.pretty {
                result += "\n";
            }

            let indent = create_indent(encode_options, depth + 1);
            let mut is_first = true;
            for (key, (expr, _)) in map {
                if is_first {
                    is_first = false;
                } else {
                    result += ",";
                    if encode_options.pretty {
                        result += "\n";
                    }
                }

                if encode_options.pretty {
                    result += &indent;
                }

                result += &format!("\"{}\"", escape_str(key));
                result += ":";
                if encode_options.pretty {
                    result += " ";
                }
                result += &encode_rec(expr, encode_options, depth + 1)?;
            }

            if encode_options.pretty {
                result += "\n";
                result += &create_indent(encode_options, depth);
            }

            result += "}";

            Ok(result)
        }
        Syntax::Lst(lst) if lst.is_empty() => Ok("[]".to_string()),
        Syntax::Lst(lst) => {
            let mut result = String::with_capacity(
                64 * lst.len()
                    + if encode_options.pretty {
                        encode_options.indent.len() * lst.len()
                    } else {
                        0
                    },
            );

            result += "[";
            if encode_options.pretty {
                result += "\n";
            }

            let indent = create_indent(encode_options, depth + 1);
            let mut is_first = true;
            for expr in lst {
                if is_first {
                    is_first = false;
                } else {
                    result += ",";
                    if encode_options.pretty {
                        result += "\n";
                    }
                }

                if encode_options.pretty {
                    result += &indent;
                }

                result += &encode_rec(expr, encode_options, depth + 1)?;
            }

            if encode_options.pretty {
                result += "\n";
                result += &create_indent(encode_options, depth);
            }

            result += "]";

            Ok(result)
        }
        Syntax::ValStr(s) => Ok(format!("\"{}\"", escape_str(s))),
        Syntax::ValInt(i) => Ok(i.to_string()),
        Syntax::ValFlt(f) => {
            let f: BigDecimal = f.clone().into();
            let result = f.to_string();
            if result.contains(".") {
                let result = result.trim_end_matches("0");
                if result.ends_with(".") {
                    Ok(format!("{}0", result))
                } else {
                    Ok(result.to_string())
                }
            } else {
                Ok(format!("{}.0", result))
            }
        }
        Syntax::ValAtom(s) if s == "true" => Ok(s),
        Syntax::ValAtom(s) if s == "false" => Ok(s),
        Syntax::ValAtom(s) if s == "none" => Ok("null".to_string()),
        expr => Err(EncodeError::UnexpectedExpr(expr)),
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum DecodeError {
    UnexpectedCharacter,
    UnexpectedEnd,
    ExpectedComma,
    ExpectedValidJSONDataType,
    ExpectedString,
    ExpectedValueSeparator,
    NumberTooBig,
    ExpectedFraction,
    ExpectedExponent,
    ExpectedEnd,
    ReachedDepthLimit,
    InvalidUnicodeEscapeSequence,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct DecodeOptions {
    /// - `None`: No limit
    /// - `Some(0)` just one layer (values like int, or an empty map or array)
    /// - `Some(1)` one layer or one layer in a list or array
    /// - `Some(x)`: See `Some(x - 1)` but just one layer deeper
    pub depth: Option<usize>,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self { depth: None }
    }
}

pub fn decode_with_options(s: &str, decode_options: &DecodeOptions) -> Result<Syntax, DecodeError> {
    let mut chars = s.char_indices().peekable();
    let result = decode_rec(&mut chars, decode_options, 0)?;
    skip_whitespace(&mut chars);
    if chars.next() == None {
        Ok(result)
    } else {
        Err(DecodeError::ExpectedEnd)
    }
}

pub fn decode(s: &str) -> Result<Syntax, DecodeError> {
    decode_with_options(s, &Default::default())
}

fn skip_whitespace<'a>(chars: &mut Peekable<CharIndices<'a>>) {
    while let Some((_, c)) = chars.peek() {
        if !c.is_whitespace() {
            break;
        }

        chars.next();
    }
}

fn is_e<'a>(chars: &mut Peekable<CharIndices<'a>>) -> bool {
    match chars.peek() {
        Some((_, 'e')) => true,
        Some((_, 'E')) => true,
        _ => false,
    }
}

fn eat_number<'a>(
    chars: &mut Peekable<CharIndices<'a>>,
    is_positive: bool,
    first: Option<char>,
) -> Result<Syntax, DecodeError> {
    let is_positive = if is_positive {
        BigInt::from_i8(1).unwrap()
    } else {
        BigInt::from_i8(-1).unwrap()
    };

    let nat: u32 = first
        .map(|digit| digit.to_string().parse().unwrap())
        .unwrap_or(0);
    let mut nat = BigInt::new(Sign::Plus, vec![nat]);
    let mut nat_digits = first.map(|_| 1).unwrap_or(0);

    // int = zero / ( digit1-9 *DIGIT )
    let mut is_first_zero = |chars: &mut Peekable<CharIndices<'_>>| {
        if Some('0') == first {
            true
        } else if nat_digits == 0 {
            if let Some((_, '0')) = chars.peek() {
                chars.next(); // eat digit
                nat_digits += 1;
                true
            } else {
                false
            }
        } else {
            false
        }
    };

    if !is_first_zero(chars) {
        while let Some((_, digit @ ('0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9'))) =
            chars.peek().cloned()
        {
            chars.next(); // eat digit
            nat *= 10;
            nat += digit.to_string().parse::<u32>().unwrap();
            nat_digits += 1;
        }
    }

    let eat_exp = |chars: &mut Peekable<CharIndices<'_>>| {
        if is_e(chars) {
            chars.next(); // eat e

            let mut result: i32 = 0;
            let mut digits = 0;

            match chars.peek() {
                Some((_, '+')) => {
                    chars.next();
                }
                Some((_, '-')) => {
                    chars.next();
                    result *= -1;
                }
                _ => {}
            }

            while let Some((
                _,
                digit @ ('0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9'),
            )) = chars.peek().cloned()
            {
                chars.next(); // eat digit

                if digits > 0 {
                    result = result.checked_mul(10).ok_or(DecodeError::NumberTooBig)?;
                }

                let digit: i32 = digit.to_string().parse::<i32>().unwrap();
                result = result.checked_add(digit).ok_or(DecodeError::NumberTooBig)?;
                digits += 1;
            }

            return Ok(result);
        }

        return Ok(0);
    };

    // frac
    if let Some((_, '.')) = chars.peek() {
        chars.next(); // eat .
        let mut frac_digits = 0;
        let mut frac = BigInt::new(Sign::Plus, vec![0]);
        while let Some((_, digit @ ('0' | '1' | '2' | '3' | '4' | '5' | '6' | '7' | '8' | '9'))) =
            chars.peek().cloned()
        {
            chars.next(); // eat digit
            frac *= 10;
            frac += digit.to_string().parse::<u32>().unwrap();
            frac_digits += 1;
        }

        if frac_digits == 0 {
            return Err(DecodeError::ExpectedFraction);
        }

        // exp
        let exp = eat_exp(chars)?;

        let mut result = (BigRational::new(nat, 1)
            + BigRational::new(
                frac,
                10_u32
                    .checked_pow(frac_digits)
                    .ok_or(DecodeError::NumberTooBig)?,
            ))
            * is_positive.into();
        if exp < 0 {
            for _ in exp..0 {
                result = result / BigRational::from_u32(10).unwrap();
            }
        } else if exp >= 1 {
            for _ in 0..exp {
                result = result * BigRational::from_u32(10).unwrap();
            }
        }

        Ok(Syntax::ValFlt(result))
    } else {
        // exp
        let exp = eat_exp(chars)?;
        if exp == 0 {
            Ok(Syntax::ValInt(nat * is_positive))
        } else if exp < 0 {
            Ok(Syntax::ValFlt(
                BigRational::new(
                    nat,
                    10_u32
                        .checked_pow(-exp as u32)
                        .ok_or(DecodeError::NumberTooBig)?,
                ) * is_positive.into(),
            ))
        } else {
            let mut nat = nat;
            for _ in 0..exp {
                nat *= 10;
            }

            Ok(Syntax::ValInt(nat * is_positive))
        }
    }
}

fn eat_value_separator<'a>(chars: &mut Peekable<CharIndices<'a>>) -> Result<(), DecodeError> {
    skip_whitespace(chars);

    match chars.next() {
        Some((_, ':')) => Ok(()),
        Some(_) | None => Err(DecodeError::ExpectedValueSeparator),
    }
}

fn eat_escape_sequence<'a>(
    chars: &mut Peekable<CharIndices<'a>>,
    result: &mut String,
) -> Result<(), DecodeError> {
    match chars.next() {
        Some((_, c @ ('"' | '\\' | '/'))) => {
            result.extend([c]);

            Ok(())
        }
        Some((_, 'b')) => {
            result.extend(['\u{0008}']);

            Ok(())
        }
        Some((_, 'f')) => {
            result.extend(['\u{000C}']);

            Ok(())
        }
        Some((_, 'n')) => {
            result.extend(['\u{000A}']);

            Ok(())
        }
        Some((_, 'r')) => {
            result.extend(['\u{000D}']);

            Ok(())
        }
        Some((_, 't')) => {
            result.extend(['\u{0009}']);

            Ok(())
        }
        Some((_, 'u')) => {
            let code = eat_unicode_hex(chars)?;
            if code & 0xDA00 != 0 {
                match chars.next() {
                    Some((_, '\\')) => match chars.peek() {
                        Some((_, 'u')) => {
                            chars.next(); // eat u

                            let code0 = code;
                            let code1 = eat_unicode_hex(chars)?;

                            let code = ((((code0 & 0x03FF) as u32) << 10)
                                | ((code1 & 0x03FF) as u32))
                                + 0xFFFF
                                + 1;

                            result.extend([char::from_u32(code as u32)
                                .ok_or(DecodeError::InvalidUnicodeEscapeSequence)?]);

                            Ok(())
                        }
                        _ => Err(DecodeError::InvalidUnicodeEscapeSequence),
                    },
                    _ => Err(DecodeError::InvalidUnicodeEscapeSequence),
                }
            } else {
                result
                    .extend([char::from_u32(code as u32)
                        .ok_or(DecodeError::InvalidUnicodeEscapeSequence)?]);

                Ok(())
            }
        }
        Some(_) => Err(DecodeError::UnexpectedCharacter),
        None => Err(DecodeError::UnexpectedEnd),
    }
}

fn eat_unicode_hex<'a>(chars: &mut Peekable<CharIndices<'a>>) -> Result<u16, DecodeError> {
    let chars: String = [chars.next(), chars.next(), chars.next(), chars.next()]
        .into_iter()
        .map(|c| {
            if let Some((_, c)) = c {
                if c.is_ascii_hexdigit() {
                    Some(c)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .try_collect()
        .ok_or(DecodeError::ExpectedString)?;

    // We already made sure chars contains only hexadecimal digits
    Ok(u16::from_str_radix(chars.as_str(), 16).unwrap())
}

fn eat_string<'a>(chars: &mut Peekable<CharIndices<'a>>) -> Result<String, DecodeError> {
    let mut result = String::new();
    loop {
        match chars.next() {
            Some((_, '"')) => {
                break;
            }
            Some((_, '\\')) => {
                eat_escape_sequence(chars, &mut result)?;
            }
            Some((_, c)) => {
                result.extend([c]);
            }
            None => {
                return Err(DecodeError::UnexpectedEnd);
            }
        }
    }

    Ok(result)
}

fn decode_rec<'a>(
    chars: &mut Peekable<CharIndices<'a>>,
    decode_options: &DecodeOptions,
    depth: usize,
) -> Result<Syntax, DecodeError> {
    if let Some(max_depth) = decode_options.depth {
        if max_depth < depth {
            return Err(DecodeError::ReachedDepthLimit);
        }
    }

    let skip_to_comma = |chars: &mut Peekable<CharIndices<'_>>| {
        skip_whitespace(chars);

        match chars.peek() {
            Some((_, ',')) => {
                chars.next();

                Ok(true)
            }
            Some((_, '}' | ']')) => Ok(false),
            Some(_) => Err(DecodeError::UnexpectedCharacter),
            None => Err(DecodeError::UnexpectedEnd),
        }
    };

    skip_whitespace(chars);
    match chars.next() {
        Some((_, '{')) => {
            skip_whitespace(chars);

            let mut result = BTreeMap::new();
            let mut skipped_comma = false;
            while chars.peek().map(|(_, c)| *c) == Some('"') {
                chars.next(); // eat "
                let key = eat_string(chars)?;
                eat_value_separator(chars)?;
                skip_whitespace(chars);
                let val = decode_rec(chars, decode_options, depth + 1)?;
                skipped_comma = skip_to_comma(chars)?;
                skip_whitespace(chars);

                let is_id = match Token::lex_for_rowan(key.as_str()) {
                    Ok(lex_result) if lex_result.len() == 1 => {
                        lex_result[0].0 == Token::Id(key.clone())
                    }
                    _ => false,
                };

                result.insert(key, (val, is_id));

                if !skipped_comma {
                    break;
                }
            }

            match chars.next() {
                Some((_, '}')) => {
                    if skipped_comma {
                        Err(DecodeError::UnexpectedCharacter)
                    } else {
                        Ok(Syntax::Map(result))
                    }
                }
                Some(_) => Err(DecodeError::UnexpectedCharacter),
                _ => Err(DecodeError::UnexpectedEnd),
            }
        }
        Some((_, '[')) => {
            skip_whitespace(chars);

            let is_end = |chars: &mut Peekable<CharIndices<'_>>| match chars.peek() {
                Some((_, ']')) | None => true,
                _ => false,
            };

            let mut result = Vec::new();
            let mut skipped_comma = false;
            while !is_end(chars) {
                result.push(decode_rec(chars, decode_options, depth + 1)?);
                skipped_comma = skip_to_comma(chars)?;
                if !skipped_comma {
                    break;
                }
            }

            match chars.next() {
                Some((_, ']')) => {
                    if skipped_comma {
                        Err(DecodeError::UnexpectedCharacter)
                    } else {
                        Ok(Syntax::Lst(result))
                    }
                }
                Some(_) => Err(DecodeError::UnexpectedCharacter),
                _ => Err(DecodeError::UnexpectedEnd),
            }
        }
        Some((_, '"')) => Ok(Syntax::ValStr(eat_string(chars)?)),
        Some((_, 'n')) => {
            if let (Some((_, c0)), Some((_, c1)), Some((_, c2))) =
                (chars.next(), chars.next(), chars.next())
            {
                if let ('u', 'l', 'l') = (c0, c1, c2) {
                    Ok(Syntax::ValAtom("none".to_string()))
                } else {
                    Err(DecodeError::ExpectedValidJSONDataType)
                }
            } else {
                Err(DecodeError::ExpectedValidJSONDataType)
            }
        }
        Some((_, 't')) => {
            if let (Some((_, c0)), Some((_, c1)), Some((_, c2))) =
                (chars.next(), chars.next(), chars.next())
            {
                if let ('r', 'u', 'e') = (c0, c1, c2) {
                    Ok(true.into())
                } else {
                    Err(DecodeError::ExpectedValidJSONDataType)
                }
            } else {
                Err(DecodeError::ExpectedValidJSONDataType)
            }
        }
        Some((_, 'f')) => {
            if let (Some((_, c0)), Some((_, c1)), Some((_, c2)), Some((_, c3))) =
                (chars.next(), chars.next(), chars.next(), chars.next())
            {
                if let ('a', 'l', 's', 'e') = (c0, c1, c2, c3) {
                    Ok(false.into())
                } else {
                    Err(DecodeError::ExpectedValidJSONDataType)
                }
            } else {
                Err(DecodeError::ExpectedValidJSONDataType)
            }
        }
        Some((_, n)) if n.is_ascii_digit() => eat_number(chars, true, Some(n)),
        Some((_, '-')) => eat_number(chars, false, None),
        Some(_) => Err(DecodeError::ExpectedValidJSONDataType),
        None => Err(DecodeError::UnexpectedEnd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execute::Syntax;

    #[test]
    fn encode_empty_map() {
        assert_eq!(
            Ok("{}".to_string()),
            encode(Syntax::Map([].into_iter().collect()))
        );
        assert_eq!(
            Ok("{}".to_string()),
            encode_pretty(Syntax::Map([].into_iter().collect()))
        );
    }

    #[test]
    fn encode_map() {
        assert_eq!(
            Ok("{\"test\":0}".to_string()),
            encode(Syntax::Map(
                [("test".to_string(), (Syntax::ValInt(0.into()), true))]
                    .into_iter()
                    .collect()
            ))
        );
        assert_eq!(
            Ok("{\n  \"test\": 0\n}".to_string()),
            encode_pretty(Syntax::Map(
                [("test".to_string(), (Syntax::ValInt(0.into()), true))]
                    .into_iter()
                    .collect()
            ))
        );
        assert_eq!(
            Ok("{\"test0\":0,\"test1\":1}".to_string()),
            encode(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true))
                ]
                .into_iter()
                .collect()
            ))
        );
        assert_eq!(
            Ok("{\n  \"test0\": 0,\n  \"test1\": 1\n}".to_string()),
            encode_pretty(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true))
                ]
                .into_iter()
                .collect()
            ))
        );
        assert_eq!(
            Ok("{\n  \"test0\": 0,\n  \"test1\": 1,\n  \"test2\": \"val\"\n}".to_string()),
            encode_pretty(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true)),
                    (
                        "test2".to_string(),
                        (Syntax::ValStr("val".to_string()), true)
                    )
                ]
                .into_iter()
                .collect()
            ))
        );
        assert_eq!(
            Ok("{\"test0\":0,\"test1\":1,\"test2\":{\"test3\":3}}".to_string()),
            encode(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true)),
                    (
                        "test2".to_string(),
                        (
                            Syntax::Map(
                                [("test3".to_string(), (Syntax::ValInt(3.into()), true))]
                                    .into_iter()
                                    .collect()
                            ),
                            true
                        )
                    )
                ]
                .into_iter()
                .collect()
            ))
        );
        assert_eq!(
            Ok("{\"test0\":0,\"test1\":1,\"test2\":{\"test3\":3,\"test4\":4}}".to_string()),
            encode(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true)),
                    (
                        "test2".to_string(),
                        (
                            Syntax::Map(
                                [
                                    ("test3".to_string(), (Syntax::ValInt(3.into()), true)),
                                    ("test4".to_string(), (Syntax::ValInt(4.into()), true))
                                ]
                                .into_iter()
                                .collect()
                            ),
                            true
                        )
                    )
                ]
                .into_iter()
                .collect()
            ))
        );
    }

    #[test]
    fn encode_empty_array() {
        assert_eq!(
            Ok("[]".to_string()),
            encode(Syntax::Lst([].into_iter().collect()))
        );
    }

    #[test]
    fn encode_array() {
        assert_eq!(
            Ok("[0]".to_string()),
            encode(Syntax::Lst(
                [Syntax::ValInt(0.into())].into_iter().collect()
            ))
        );

        assert_eq!(
            Ok("[0,1]".to_string()),
            encode(Syntax::Lst(
                [Syntax::ValInt(0.into()), Syntax::ValInt(1.into())]
                    .into_iter()
                    .collect()
            ))
        );

        assert_eq!(
            Ok("[0,1,[0,2]]".to_string()),
            encode(Syntax::Lst(
                [
                    Syntax::ValInt(0.into()),
                    Syntax::ValInt(1.into()),
                    Syntax::Lst(
                        [Syntax::ValInt(0.into()), Syntax::ValInt(2.into())]
                            .into_iter()
                            .collect()
                    )
                ]
                .into_iter()
                .collect()
            ))
        );
    }

    #[test]
    fn encode_int() {
        assert_eq!(Ok("0".to_string()), encode(Syntax::ValInt(0.into())));
        assert_eq!(Ok("1".to_string()), encode(Syntax::ValInt(1.into())));
    }

    #[test]
    fn encode_flt() {
        assert_eq!(
            Ok("0.0".to_string()),
            encode(Syntax::ValFlt(BigRational::from_u32(0).unwrap()))
        );
        assert_eq!(
            Ok("1.0".to_string()),
            encode(Syntax::ValFlt(BigRational::from_u32(1).unwrap()))
        );
        assert_eq!(
            Ok("1.1".to_string()),
            encode(Syntax::ValFlt(BigRational::from_f32(1.1).unwrap()))
        );
        assert_eq!(
            Ok("12.413".to_string()),
            encode(Syntax::ValFlt(BigRational::from_f32(12.413).unwrap()))
        );
        assert_eq!(
            Ok("-12.413".to_string()),
            encode(Syntax::ValFlt(BigRational::from_f32(-12.413).unwrap()))
        );
    }

    #[test]
    fn encode_keywords() {
        assert_eq!(
            Ok("true".to_string()),
            encode(Syntax::ValAtom("true".to_string()))
        );
        assert_eq!(
            Ok("false".to_string()),
            encode(Syntax::ValAtom("false".to_string()))
        );
        assert_eq!(
            Ok("null".to_string()),
            encode(Syntax::ValAtom("none".to_string()))
        );
    }

    #[test]
    fn encode_string() {
        assert_eq!(
            Ok("\"\\\"\"".to_string()),
            encode(Syntax::ValStr("\"".to_string()))
        );
        assert_eq!(
            Ok("\"\\\\\"".to_string()),
            encode(Syntax::ValStr("\\".to_string()))
        );
        assert_eq!(
            Ok("\"\\r\"".to_string()),
            encode(Syntax::ValStr("\r".to_string()))
        );
        assert_eq!(
            Ok("\"\\n\"".to_string()),
            encode(Syntax::ValStr("\n".to_string()))
        );
        assert_eq!(
            Ok("\"\\t\"".to_string()),
            encode(Syntax::ValStr("\t".to_string()))
        );
        assert_eq!(
            Ok("\"\\b\"".to_string()),
            encode(Syntax::ValStr("\u{0008}".to_string()))
        );
        assert_eq!(
            Ok("\"\\f\"".to_string()),
            encode(Syntax::ValStr("\u{000C}".to_string()))
        );
    }

    #[test]
    fn decode_empty_map() {
        assert_eq!(Ok(Syntax::Map(Default::default())), decode("{}"));
        assert_eq!(Ok(Syntax::Map(Default::default())), decode("         {}"));
        assert_eq!(
            Ok(Syntax::Map(Default::default())),
            decode("         {}              ")
        );
        assert_eq!(
            Ok(Syntax::Map(Default::default())),
            decode("         {              }              ")
        );
    }

    #[test]
    fn decode_map() {
        assert_eq!(
            Ok(Syntax::Map(
                [("test".to_string(), (Syntax::ValInt(0.into()), true))]
                    .into_iter()
                    .collect()
            )),
            decode("{ \"test\": 0 }")
        );
        assert_eq!(
            Ok(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true))
                ]
                .into_iter()
                .collect()
            )),
            decode("{ \"test0\": 0, \"test1\": 1 }")
        );
        assert_eq!(
            Ok(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true))
                ]
                .into_iter()
                .collect()
            )),
            decode("{\"test0\":0,\"test1\":1}")
        );
        assert_eq!(
            Ok(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true)),
                    ("test2".to_string(), (Syntax::ValInt(2.into()), true))
                ]
                .into_iter()
                .collect()
            )),
            decode("{\"test0\":0,\"test1\":1,\"test2\":2}")
        );
        assert_eq!(
            Ok(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true)),
                    ("test2".to_string(), (Syntax::ValInt(2.into()), true))
                ]
                .into_iter()
                .collect()
            )),
            decode("   {   \"test0\":    0    ,   \"test1\"   :    1   ,\"test2\":2    }")
        );
        assert_eq!(
            Ok(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true)),
                    ("test2".to_string(), (Syntax::ValInt(2.into()), true)),
                    (
                        "test3".to_string(),
                        (
                            Syntax::Map(
                                [(
                                    "key".to_string(),
                                    (Syntax::ValStr("value".to_string()), true)
                                )]
                                .into_iter()
                                .collect()
                            ),
                            true
                        )
                    )
                ]
                .into_iter()
                .collect()
            )),
            decode("{\"test0\":0,\"test1\":1,\"test2\":2,\"test3\":{\"key\": \"value\"}}")
        );
        assert_eq!(
            Ok(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("test1".to_string(), (Syntax::ValInt(1.into()), true)),
                    (
                        "test2".to_string(),
                        (
                            Syntax::Lst([Syntax::ValStr("val".to_string())].into_iter().collect()),
                            true
                        )
                    )
                ]
                .into_iter()
                .collect()
            )),
            decode("{\"test0\":0,\"test1\":1,\"test2\":[\"val\"]}")
        );
        assert_eq!(
            Ok(Syntax::Map(
                [
                    ("test0".to_string(), (Syntax::ValInt(0.into()), true)),
                    ("1test1".to_string(), (Syntax::ValInt(1.into()), false)),
                    (
                        "test2".to_string(),
                        (
                            Syntax::Lst([Syntax::ValStr("val".to_string())].into_iter().collect()),
                            true
                        )
                    )
                ]
                .into_iter()
                .collect()
            )),
            decode("{\"test0\":0,\"1test1\":1,\"test2\":[\"val\"]}")
        );
    }

    #[test]
    fn decode_map_invalid() {
        assert_eq!(
            Err(DecodeError::UnexpectedCharacter),
            decode("{\"test0\":0,\"test1\":1,,\"test2\":2}")
        );
        assert_eq!(
            Err(DecodeError::UnexpectedCharacter),
            decode("{\"test0\":0,\"test1\":1\"test2\":2}")
        );
        assert_eq!(
            Err(DecodeError::ExpectedValidJSONDataType),
            decode("{\"test0\"::0,\"test1\":1,\"test2\":2}")
        );
        assert_eq!(
            Err(DecodeError::ExpectedValidJSONDataType),
            decode("{\"test0\":}")
        );
        assert_eq!(
            Err(DecodeError::UnexpectedCharacter),
            decode("{\"test0\": 0,}")
        );
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("{"));
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("{\""));
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("{\"test0\":"));
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("{\"test0\":0"));
        assert_eq!(
            Err(DecodeError::ExpectedValueSeparator),
            decode("{\"test0\"")
        );
    }

    #[test]
    fn decode_empty_array() {
        assert_eq!(Ok(Syntax::Lst(Default::default())), decode("[]"));
        assert_eq!(Ok(Syntax::Lst(Default::default())), decode("         []"));
        assert_eq!(
            Ok(Syntax::Lst(Default::default())),
            decode("[]             ")
        );
        assert_eq!(
            Ok(Syntax::Lst(Default::default())),
            decode("[              ]")
        );
        assert_eq!(
            Ok(Syntax::Lst(Default::default())),
            decode("  [              ]")
        );
        assert_eq!(
            Ok(Syntax::Lst(Default::default())),
            decode("[              ]   ")
        );
    }

    #[test]
    fn decode_array() {
        assert_eq!(
            Ok(Syntax::Lst(
                [Syntax::ValInt(0.into())].into_iter().collect()
            )),
            decode("[0]")
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [Syntax::ValInt(0.into()), Syntax::ValInt(1.into())]
                    .into_iter()
                    .collect()
            )),
            decode("[0, 1]")
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [
                    Syntax::ValInt(0.into()),
                    Syntax::ValInt(1.into()),
                    Syntax::ValInt(2.into())
                ]
                .into_iter()
                .collect()
            )),
            decode("[0, 1, 2]")
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [
                    Syntax::ValInt(0.into()),
                    Syntax::ValInt(1.into()),
                    Syntax::ValInt(2.into())
                ]
                .into_iter()
                .collect()
            )),
            decode("[0,1,2]")
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [
                    Syntax::ValInt(0.into()),
                    Syntax::ValInt(2.into()),
                    Syntax::ValInt(1.into())
                ]
                .into_iter()
                .collect()
            )),
            decode("[0,2,1]")
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [
                    Syntax::ValInt(0.into()),
                    Syntax::ValInt(2.into()),
                    Syntax::Map([].into_iter().collect())
                ]
                .into_iter()
                .collect()
            )),
            decode("[0,2,{}]")
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [
                    Syntax::ValInt(0.into()),
                    Syntax::ValInt(2.into()),
                    Syntax::Lst([Syntax::ValInt(0.into()),].into_iter().collect())
                ]
                .into_iter()
                .collect()
            )),
            decode("[0,2,[0]]")
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [
                    false.into(),
                    true.into(),
                    Syntax::ValAtom("none".to_string())
                ]
                .into_iter()
                .collect()
            )),
            decode("[false,true,null]")
        );
    }

    #[test]
    fn decode_array_invalid() {
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("["));
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("[0"));
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("[0,"));
        assert_eq!(Err(DecodeError::UnexpectedEnd), decode("[0,[]"));
    }

    #[test]
    fn decode_empty_string() {
        assert_eq!(Ok(Syntax::ValStr("".to_string())), decode("\"\""));
    }

    #[test]
    fn decode_int() {
        assert_eq!(Ok(Syntax::ValInt(0.into())), decode("0"));
        assert_eq!(Ok(Syntax::ValInt(1.into())), decode("1"));
        assert_eq!(Ok(Syntax::ValInt(2.into())), decode("2"));
        assert_eq!(Ok(Syntax::ValInt(3.into())), decode("3"));
        assert_eq!(Ok(Syntax::ValInt(4.into())), decode("4"));
        assert_eq!(Ok(Syntax::ValInt(5.into())), decode("5"));
        assert_eq!(Ok(Syntax::ValInt(6.into())), decode("6"));
        assert_eq!(Ok(Syntax::ValInt(7.into())), decode("7"));
        assert_eq!(Ok(Syntax::ValInt(8.into())), decode("8"));
        assert_eq!(Ok(Syntax::ValInt(9.into())), decode("9"));
        assert_eq!(Ok(Syntax::ValInt(10.into())), decode("10"));
        assert_eq!(Ok(Syntax::ValInt(11.into())), decode("11"));
        assert_eq!(Ok(Syntax::ValInt(12.into())), decode("12"));
        assert_eq!(Ok(Syntax::ValInt(13.into())), decode("13"));
        assert_eq!(Ok(Syntax::ValInt(14.into())), decode("14"));
        assert_eq!(Ok(Syntax::ValInt(15.into())), decode("15"));
        assert_eq!(Ok(Syntax::ValInt(16.into())), decode("16"));
        assert_eq!(Ok(Syntax::ValInt(17.into())), decode("17"));
        assert_eq!(Ok(Syntax::ValInt(18.into())), decode("18"));
        assert_eq!(Ok(Syntax::ValInt(19.into())), decode("19"));
        assert_eq!(Ok(Syntax::ValInt(190.into())), decode("190"));
        assert_eq!(Ok(Syntax::ValInt(195.into())), decode("195"));
        assert_eq!(Ok(Syntax::ValInt(195.into())), decode("195e0"));
        assert_eq!(Ok(Syntax::ValInt(1950.into())), decode("195e1"));
        assert_eq!(Ok(Syntax::ValInt(19500.into())), decode("195e2"));
        assert_eq!(
            Ok(Syntax::ValInt(
                BigRational::from_f64(195e11).unwrap().split().0
            )),
            decode("195e11")
        );
    }

    #[test]
    fn decode_negative_int() {
        assert_eq!(Ok(Syntax::ValInt((-0).into())), decode("-0"));
        assert_eq!(Ok(Syntax::ValInt((-1).into())), decode("-1"));
        assert_eq!(Ok(Syntax::ValInt((-2).into())), decode("-2"));
        assert_eq!(Ok(Syntax::ValInt((-3).into())), decode("-3"));
        assert_eq!(Ok(Syntax::ValInt((-4).into())), decode("-4"));
        assert_eq!(Ok(Syntax::ValInt((-5).into())), decode("-5"));
        assert_eq!(Ok(Syntax::ValInt((-6).into())), decode("-6"));
        assert_eq!(Ok(Syntax::ValInt((-7).into())), decode("-7"));
        assert_eq!(Ok(Syntax::ValInt((-8).into())), decode("-8"));
        assert_eq!(Ok(Syntax::ValInt((-9).into())), decode("-9"));
        assert_eq!(Ok(Syntax::ValInt((-10).into())), decode("-10"));
        assert_eq!(Ok(Syntax::ValInt((-11).into())), decode("-11"));
        assert_eq!(Ok(Syntax::ValInt((-12).into())), decode("-12"));
        assert_eq!(Ok(Syntax::ValInt((-13).into())), decode("-13"));
        assert_eq!(Ok(Syntax::ValInt((-14).into())), decode("-14"));
        assert_eq!(Ok(Syntax::ValInt((-15).into())), decode("-15"));
        assert_eq!(Ok(Syntax::ValInt((-16).into())), decode("-16"));
        assert_eq!(Ok(Syntax::ValInt((-17).into())), decode("-17"));
        assert_eq!(Ok(Syntax::ValInt((-18).into())), decode("-18"));
        assert_eq!(Ok(Syntax::ValInt((-19).into())), decode("-19"));
    }

    #[test]
    fn decode_flt() {
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.0).unwrap())),
            decode("0.0")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.1).unwrap())),
            decode("0.1")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.2).unwrap())),
            decode("0.2")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.3).unwrap())),
            decode("0.3")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.4).unwrap())),
            decode("0.4")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.5).unwrap())),
            decode("0.5")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.6).unwrap())),
            decode("0.6")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.7).unwrap())),
            decode("0.7")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.8).unwrap())),
            decode("0.8")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(0.9).unwrap())),
            decode("0.9")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(1.9).unwrap())),
            decode("1.9")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(2.9).unwrap())),
            decode("2.9")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(3.9).unwrap())),
            decode("3.9")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(4.9).unwrap())),
            decode("4.9")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(41.9).unwrap())),
            decode("41.9")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(41.954).unwrap())),
            decode("41.954")
        );
    }

    #[test]
    fn decode_negative_flt() {
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.0).unwrap())),
            decode("-0.0")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.1).unwrap())),
            decode("-0.1")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.2).unwrap())),
            decode("-0.2")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.3).unwrap())),
            decode("-0.3")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.4).unwrap())),
            decode("-0.4")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.5).unwrap())),
            decode("-0.5")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.6).unwrap())),
            decode("-0.6")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.7).unwrap())),
            decode("-0.7")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.8).unwrap())),
            decode("-0.8")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-0.9).unwrap())),
            decode("-0.9")
        );
        assert_eq!(
            Ok(Syntax::ValFlt(BigRational::from_f64(-1.9).unwrap())),
            decode("-1.9")
        );
    }

    #[test]
    fn decode_string() {
        assert_eq!(Ok(Syntax::ValStr(" ".to_string())), decode("\" \""));
        assert_eq!(Ok(Syntax::ValStr("a".to_string())), decode("\"a\""));
        assert_eq!(Ok(Syntax::ValStr("b".to_string())), decode("\"b\""));

        // Escape sequence
        assert_eq!(Ok(Syntax::ValStr("\\".to_string())), decode("\"\\\\\""));
        assert_eq!(Ok(Syntax::ValStr("\"".to_string())), decode("\"\\\"\""));
        assert_eq!(
            Ok(Syntax::ValStr("\u{0008}".to_string())),
            decode("\"\\b\"")
        );
        assert_eq!(
            Ok(Syntax::ValStr("\u{000C}".to_string())),
            decode("\"\\f\"")
        );
        assert_eq!(Ok(Syntax::ValStr("\n".to_string())), decode("\"\\n\""));
        assert_eq!(Ok(Syntax::ValStr("\r".to_string())), decode("\"\\r\""));
        assert_eq!(Ok(Syntax::ValStr("\t".to_string())), decode("\"\\t\""));
        assert_eq!(Ok(Syntax::ValStr("ß".to_string())), decode("\"\\u00DF\""));
        assert_eq!(
            Ok(Syntax::ValStr("\u{1D11E}".to_string())),
            decode("\"\\uD834\\uDD1E\"")
        );

        assert_eq!(Ok(Syntax::ValStr("e\"f".to_string())), decode("\"e\\\"f\""));
        assert_eq!(
            Ok(Syntax::ValStr("ee\"f".to_string())),
            decode("\"ee\\\"f\"")
        );
    }

    #[test]
    fn decode_expect_end() {
        assert_eq!(Err(DecodeError::ExpectedEnd), decode("[] []"));
    }

    #[test]
    fn decode_keywords() {
        assert_eq!(Ok(Syntax::ValAtom("true".to_string())), decode("true"));
        assert_eq!(Ok(Syntax::ValAtom("false".to_string())), decode("false"));
        assert_eq!(Ok(Syntax::ValAtom("none".to_string())), decode("null"));
    }

    #[test]
    fn decode_keywords_invalid() {
        assert_eq!(Err(DecodeError::ExpectedValidJSONDataType), decode("tru"));
        assert_eq!(Err(DecodeError::ExpectedValidJSONDataType), decode("fale"));
        assert_eq!(Err(DecodeError::ExpectedValidJSONDataType), decode("nul"));
    }

    #[test]
    fn decode_with_options_depth() {
        assert_eq!(
            Ok(Syntax::ValInt(0.into())),
            decode_with_options(
                "0",
                &DecodeOptions {
                    depth: Some(0),
                    ..Default::default()
                }
            )
        );
        assert_eq!(
            Ok(Syntax::Map([].into_iter().collect())),
            decode_with_options(
                "{}",
                &DecodeOptions {
                    depth: Some(0),
                    ..Default::default()
                }
            )
        );
        assert_eq!(
            Err(DecodeError::ReachedDepthLimit),
            decode_with_options(
                "{\"test\": 0}",
                &DecodeOptions {
                    depth: Some(0),
                    ..Default::default()
                }
            )
        );
        assert_eq!(
            Ok(Syntax::Map(
                [("test".to_string(), (Syntax::ValInt(0.into()), true))]
                    .into_iter()
                    .collect()
            )),
            decode_with_options(
                "{\"test\": 0}",
                &DecodeOptions {
                    depth: Some(1),
                    ..Default::default()
                }
            )
        );
        assert_eq!(
            Ok(Syntax::Lst([].into_iter().collect())),
            decode_with_options(
                "[]",
                &DecodeOptions {
                    depth: Some(0),
                    ..Default::default()
                }
            )
        );
        assert_eq!(
            Err(DecodeError::ReachedDepthLimit),
            decode_with_options(
                "[0]",
                &DecodeOptions {
                    depth: Some(0),
                    ..Default::default()
                }
            )
        );
        assert_eq!(
            Ok(Syntax::Lst(
                [Syntax::ValInt(0.into())].into_iter().collect()
            )),
            decode_with_options(
                "[0]",
                &DecodeOptions {
                    depth: Some(1),
                    ..Default::default()
                }
            )
        );
    }
}
