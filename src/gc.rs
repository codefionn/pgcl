use std::collections::{HashSet, VecDeque};

use crate::{execute::Syntax, system::SystemHandler};

/// Mark all the given `exprs` as used.
///
/// Currently this only markes signals.
///
/// ## Arguments
///
/// - `system`: The system commucation handler
/// - `exprs`: Expressions to mark. All child-expressions will also be marked.
pub async fn mark_used(system: &mut SystemHandler, exprs: &[&Syntax]) {
    let mut broadsearch: VecDeque<&Syntax> = VecDeque::new();
    broadsearch.extend(exprs);
    let mut already_marked = HashSet::new();

    while let Some(syntax) = broadsearch.pop_front() {
        match syntax {
            Syntax::Signal(signal_type, id) => {
                // Only if not marked, marked as used
                if already_marked.insert((signal_type, id)) {
                    system.mark_use_signal(signal_type.clone(), *id).await;
                }
            }
            _ => broadsearch.extend(syntax.exprs().into_iter()),
        }
    }
}
