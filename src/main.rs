#![feature(box_patterns)]
#![feature(iterator_try_collect)]
#![feature(async_fn_in_trait)]

mod actor;
mod context;
mod errors;
mod execute;
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
use log::{debug, LevelFilter};
use runner::Runner;
use system::SystemHandler;
use tokio::sync::mpsc;

#[derive(Parser, Clone, Debug)]
pub struct Args {
    /// Be verbose (more messages)
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Script to execute
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    env_logger::builder()
        .filter_level(if args.verbose {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        })
        .filter_module("rustyline", LevelFilter::Warn)
        .init();

    if let Some(filepath) = args.file {
        let code = std::fs::read_to_string(&filepath)?;

        let mut holder = ContextHolder::default();
        let mut systems = SystemHandler::default();
        let mut runner = Runner::new(&mut systems).await?;

        crate::execute::execute_code(
            &filepath.to_string_lossy(),
            filepath.parent().map(|path| path.to_path_buf()),
            code.as_str(),
            &mut holder,
            &mut systems,
            &mut runner,
            args.verbose,
        )
        .await
        .map_err(|err| anyhow!("{:?}", err))?;

        systems.exit().await;

        debug!("Exiting main");
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
