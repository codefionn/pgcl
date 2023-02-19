#![feature(box_patterns)]

mod errors;
mod execute;
mod interpreter;
mod lexer;
mod parser;
mod reader;

#[cfg(test)]
mod tests;

use bevy::{
    log::{Level, LogPlugin},
    prelude::*,
};
use clap::Parser;

#[derive(Parser, Clone, Debug, Resource)]
pub struct Args {
    #[arg(short, long, default_value_t = false)]
    verbose: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    App::new()
        .insert_resource(args.clone())
        //.add_plugin(CorePlugin::default())
        .add_plugin(HierarchyPlugin::default())
        .add_plugin(LogPlugin {
            filter: "rustyline=error".to_string(),
            level: if args.verbose {
                Level::DEBUG
            } else {
                Level::INFO
            },
        })
        .add_plugin(reader::ReaderPlugin)
        .add_plugin(interpreter::InterpreterPlugin)
        .run();

    Ok(())
}
