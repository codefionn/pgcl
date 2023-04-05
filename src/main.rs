#![feature(box_patterns)]
#![feature(iterator_try_collect)]

mod actor;
mod context;
mod errors;
mod execute;
mod interpreter;
mod lexer;
mod parser;
mod rational;
mod reader;
mod system;

#[cfg(test)]
mod tests;

use clap::Parser;
use log::LevelFilter;
use tokio::sync::mpsc;

#[derive(Parser, Clone, Debug)]
pub struct Args {
    /// Be verbose (more messages)
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
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

    let (tx_cli, rx_cli) = mpsc::channel(1);
    let cli_actor = reader::CLIActor::new(tx_cli);
    let interpreter_actor = interpreter::InterpreterActor::new(args, rx_cli);

    let (h0, h1) = tokio::join!(
        tokio::spawn(cli_actor.run()),
        tokio::spawn(interpreter_actor.run())
    );

    h0??;
    h1??;

    Ok(())
}
