use assert_cmd::Command;
use predicates::prelude::*;

use test::Bencher;

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

#[bench]
fn test_bench_cli_add(b: &mut Bencher) -> anyhow::Result<()> {
    b.iter(|| {
        let mut cmd = Command::cargo_bin("pgcl").unwrap();
        cmd.write_stdin("1 + 2\n")
            .assert()
            .success()
            .stdout(predicate::str::is_match(r"^3\n$").unwrap());
    });

    Ok(())
}

#[bench]
fn test_bench_add(b: &mut Bencher) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    b.iter(move || {
        rt.block_on(async {
            assert_eq!(
                Ok(format!("5")),
                parse_to_str(
                    "2 + 3",
                    &mut ContextHandler::async_default().await,
                    &mut SystemHandler::default()
                )
                .await
            );
        });
    });
}

#[bench]
fn test_bench_pow(b: &mut Bencher) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    b.iter(move || {
        rt.block_on(async {
            assert_eq!(
                Ok(format!("16")),
                parse_to_str(
                    "2 ** 4",
                    &mut ContextHandler::async_default().await,
                    &mut SystemHandler::default()
                )
                .await
            );
        });
    });
}

#[bench]
fn test_bench_add_map(b: &mut Bencher) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    b.iter(move || {
        rt.block_on(async {
            assert_eq!(
                Ok(format!("30")),
                parse_to_str(
                    "nat x = (nat (x + 1)) + {} + [\"key\" + x, x]\nlet { key30 } = nat 0 in key30",
                    &mut ContextHandler::async_default().await,
                    &mut SystemHandler::default()
                )
                .await
            );
        });
    });
}

#[bench]
fn test_bench_std_limit(b: &mut Bencher) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    b.iter(move || {
        rt.block_on(async {
            assert_eq!(
                Ok(format!("[0, 1, 2, 3]")),
                parse_to_str(
                    "nat x = [x] + (nat (x + 1))\n(import std).limit 4 (nat 0)",
                    &mut ContextHandler::async_default().await,
                    &mut SystemHandler::default()
                )
                .await
            );
        });
    });
}
