use bevy::prelude::*;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::Editor;

pub struct ReaderPlugin;

#[derive(Resource, Clone, Debug, PartialEq)]
struct State {}

impl Plugin for ReaderPlugin {
    fn build(&self, app: &mut App) {
        app.add_startup_system(setup)
            .add_event::<Message>()
            .set_runner(|mut app| {
                let mut rl = Editor::<()>::new().unwrap();
                rl.set_auto_add_history(true);

                app.update();
                loop {
                    let readline = rl.readline("> ");

                    let msg = match readline {
                        Ok(line) => Message::Line(line),
                        Err(ReadlineError::Interrupted) => Message::Exit(),
                        Err(ReadlineError::Eof) => Message::Exit(),
                        Err(err) => {
                            println!("Error: {:?}", err);
                            Message::Exit()
                        }
                    };

                    if Message::Exit() == msg {
                        break;
                    }

                    app.world.send_event(msg);
                    app.update();
                    app.update();
                }
            });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Line(String),
    Exit(),
}

fn setup() {}
