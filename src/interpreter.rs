use bevy::prelude::*;
use rowan::GreenNodeBuilder;

use crate::{
    errors::InterpreterError,
    execute::{Context, Syntax},
    lexer::Token,
    parser::{print_ast, Parser, SyntaxElement, SyntaxKind},
    reader::Message,
    Args,
};

#[derive(SystemLabel)]
pub enum Label {
    InterpretLine,
}

pub struct InterpreterPlugin;

#[derive(Clone, Deref, Debug)]
pub struct LexerEvent(Vec<(Token, String)>);

impl Plugin for InterpreterPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<LexerEvent>()
            .add_system(lexer)
            .add_system(parse);
    }
}

fn lexer(mut ev: EventReader<crate::reader::Message>, mut evw: EventWriter<LexerEvent>) {
    for msg in ev.iter() {
        match msg {
            Message::Line(line) => {
                evw.send(LexerEvent(Token::lex_for_rowan(line.as_str())));
            }
            _ => {}
        }
    }
}

fn parse(mut ev: EventReader<LexerEvent>, _args: Res<Args>, mut state: Local<Context>) {
    for lexer_result in ev.iter() {
        let lexer_result = &**lexer_result;
        debug!("{:?}", lexer_result);

        let toks: Vec<(SyntaxKind, String)> = lexer_result
            .into_iter()
            .map(|(tok, slice)| (tok.clone().try_into().unwrap(), slice.clone()))
            .collect();

        let (ast, errors) =
            Parser::new(GreenNodeBuilder::new(), toks.into_iter().peekable()).parse();
        let ast: SyntaxElement = ast.into();
        println!("{:?}", errors);
        print_ast(0, ast.clone());

        let typed: Result<Syntax, InterpreterError> = ast.try_into();
        println!("{:?}", typed);
        if let Ok(typed) = typed {
            let reduced = typed.reduce();
            println!("{}", reduced);
            if let Ok(executed) = reduced.execute(true, &mut state) {
                println!("{}", executed);
            }
        }
    }
}
