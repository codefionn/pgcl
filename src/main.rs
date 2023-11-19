#![feature(box_patterns)]
#![feature(iterator_try_collect)]
#![feature(test)]

extern crate test;

mod actor;
mod context;
mod errors;
mod execute;
mod ext_parse;
mod gc;
mod interpreter;
mod lexer;
mod parser;
mod rational;
mod reader;
mod runner;
mod syscall;
mod system;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use anyhow::anyhow;
use clap::Parser;
use context::ContextHolder;
use errors::InterpreterError;
use log::LevelFilter;
use runner::Runner;
use system::SystemHandler;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialOrd, PartialEq, Ord, Eq)]
#[repr(u8)]
pub enum VerboseLevel {
    None,
    Statements,
    Debug,
    Trace,
}

impl Default for VerboseLevel {
    fn default() -> Self {
        Self::None
    }
}

impl From<&str> for VerboseLevel {
    fn from(value: &str) -> Self {
        match value {
            "none" => Self::None,
            "statements" => Self::Statements,
            "debug" => Self::Debug,
            "trace" => Self::Trace,
            _ => Self::None,
        }
    }
}

impl From<u8> for VerboseLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::None,
            1 => Self::Statements,
            2 => Self::Debug,
            3 => Self::Trace,
            _ => Self::None,
        }
    }
}

#[derive(Parser, Clone, Debug)]
pub struct Args {
    /// Be verbose (more messages)
    #[arg(short = 'v', global = true, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Enter debug mode
    #[arg(short, long, default_value_t = false)]
    debug: bool,

    /// Optimize standard library calls
    #[arg(short, long, default_value_t = true)]
    optimize: bool,

    /// Script to execute
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let verbose: VerboseLevel = args.verbose.into();
    env_logger::builder()
        .filter_level(match verbose {
            VerboseLevel::Debug => LevelFilter::Debug,
            VerboseLevel::Trace => LevelFilter::Trace,
            _ => LevelFilter::Info,
        })
        .filter_module("rustyline", LevelFilter::Warn)
        .init();

    if let Some(filepath) = args.file {
        let code = std::fs::read_to_string(&filepath)?;

        let mut holder = ContextHolder::default();
        let mut systems = SystemHandler::default();
        let mut runner = Runner::new(&mut systems).await?;

        let mut exit_code = 0;
        match crate::execute::execute_code(
            &filepath.to_string_lossy(),
            filepath.parent().map(|path| path.to_path_buf()),
            code.as_str(),
            &mut holder,
            &mut systems,
            &mut runner,
            verbose,
            args.debug,
        )
        .await
        {
            Ok(_) => {}
            Err(InterpreterError::ProgramTerminatedByUser(id)) => {
                exit_code = id;
            }
            Err(err) => {
                std::mem::drop(runner);
                systems.exit().await;

                return Err(anyhow!("main: {:?}", err));
            }
        };
        std::mem::drop(runner);

        if exit_code == 0 && systems.has_failed_asserts().await {
            exit_code = 1;
        }

        systems.exit().await;

        if exit_code != 0 {
            std::process::exit(exit_code);
        }
    } else {
        let (tx_cli, rx_cli) = mpsc::channel(1);
        let cli_actor = reader::CLIActor::new(tx_cli);
        let interpreter_actor = interpreter::InterpreterActor::new(args, rx_cli);

        let (h0, h1) = tokio::join!(
            tokio::spawn(cli_actor.run()),
            tokio::spawn(interpreter_actor.run())
        );

        let exit_code = h0??;
        h1??;

        if exit_code != 0 {
            std::process::exit(exit_code);
        }
    }

    Ok(())
}
