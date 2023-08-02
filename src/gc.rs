use std::collections::{VecDeque, HashSet};

use crate::{execute::Syntax, system::SystemHandler};

pub async fn mark_used(system: &mut SystemHandler, syntax: &Syntax) {
    let mut broadsearch: VecDeque<&Syntax> = VecDeque::new();
    broadsearch.push_front(syntax);
    let mut already_marked = HashSet::new();

    while let Some(syntax) = broadsearch.pop_front() {
        match syntax {
            Syntax::Signal(signal_type, id) => {
                // Nur wenn hier noch nicht markiert erneut einfügen
                if already_marked.insert((signal_type, id)) {
                    system.mark_use_signal(signal_type.clone(), *id).await;
                }
            }
            _ => {
                broadsearch.extend(syntax.exprs().into_iter())
            }
        }
    }
}
