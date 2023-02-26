use bevy::prelude::*;
use rowan::GreenNodeBuilder;

use crate::{
    errors::InterpreterError,
    execute::{BiOpType, Context, Syntax},
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

fn parse(
    mut ev: EventReader<LexerEvent>,
    args: Res<Args>,
    mut state: Local<Context>,
    mut last_result: Local<Option<Syntax>>,
) {
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
        if !errors.is_empty() {
            println!("{:?}", errors);
        }

        if args.verbose {
            print_ast(0, ast.clone());
        }

        let typed: Result<Syntax, InterpreterError> = ast.try_into();
        debug!("{:?}", typed);
        if let Ok(typed) = typed {
            let typed = match typed {
                Syntax::Pipe(expr) if last_result.is_some() => Syntax::BiOp(
                    BiOpType::OpPipe,
                    Box::new(last_result.as_ref().unwrap().clone()),
                    expr,
                ),
                _ => typed,
            };

            let reduced = typed.reduce();
            debug!("{}", reduced);
            if let Ok(executed) = reduced.execute(true, &mut state) {
                println!("{}", executed);
                *last_result = Some(executed);
            }
        }
    }
}
