//! The app-level undo register: the last few note-level destructions, each
//! held with everything its reverse needs, captured at the moment of
//! destruction (adr/2026-08-app-level-undo-register.md). Pure data — the
//! reverses themselves run in `ui`, where the signals live, so the register
//! stays testable without a VirtualDom (`adr/2026-07-ui-covered-at-100.md`).
//! Distinct from the editor's vim-grain history: that one undoes keystrokes
//! inside the open buffer; this one undoes actions on notes and cards.

use std::path::PathBuf;

/// How many intents the register keeps. In-memory and gone with the app —
/// git stays the deep recovery, per the delete decisions this register
/// completes rather than replaces.
const DEPTH: usize = 10;

/// One reversible action, holding its before-image.
#[derive(Debug, PartialEq)]
pub enum Intent {
    /// A deleted note: its text, its card's coordinates (`None` when it was
    /// never pinned), and the id the label names it by.
    Delete {
        path: PathBuf,
        id: String,
        text: String,
        position: Option<(f64, f64)>,
    },
    /// An arrange: every moved card's prior coordinates, `None` for a card
    /// that was auto-placed — its reverse is unpinning, not a move.
    Arrange {
        prior: Vec<(String, Option<(f64, f64)>)>,
    },
}

/// The stack itself. Bounded by `DEPTH`: the oldest intent falls off, it is
/// never a save-blocker or a growing ledger.
#[derive(Debug, Default)]
pub struct Register {
    stack: Vec<Intent>,
}

impl Register {
    pub fn push(&mut self, intent: Intent) {
        self.stack.push(intent);
        if self.stack.len() > DEPTH {
            self.stack.remove(0);
        }
    }

    /// The name the palette row wears: what one undo would take back.
    /// `None` is how the palette knows to hide the command.
    pub fn label(&self) -> Option<String> {
        self.stack.last().map(|intent| match intent {
            Intent::Delete { id, .. } => format!("undo delete {id}"),
            Intent::Arrange { .. } => "undo arrange".to_string(),
        })
    }

    pub fn pop(&mut self) -> Option<Intent> {
        self.stack.pop()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn delete(id: &str) -> Intent {
        Intent::Delete {
            path: PathBuf::from(format!("permanent/{id}.typ")),
            id: id.to_string(),
            text: "= contenu\n".to_string(),
            position: Some((10.0, 20.0)),
        }
    }

    #[test]
    fn the_label_names_the_last_intent_or_nothing() {
        let mut register = Register::default();
        assert_eq!(register.label(), None);
        register.push(delete("zettel"));
        assert_eq!(register.label(), Some("undo delete zettel".to_string()));
        register.push(Intent::Arrange { prior: Vec::new() });
        assert_eq!(register.label(), Some("undo arrange".to_string()));
    }

    #[test]
    fn pop_walks_the_stack_newest_first() {
        let mut register = Register::default();
        register.push(delete("premier"));
        register.push(delete("second"));
        assert_eq!(register.pop(), Some(delete("second")));
        assert_eq!(register.pop(), Some(delete("premier")));
        assert_eq!(register.pop(), None);
    }

    #[test]
    fn the_register_is_bounded_and_drops_the_oldest() {
        let mut register = Register::default();
        for n in 0..(DEPTH + 3) {
            register.push(delete(&format!("note-{n}")));
        }
        let mut popped = 0;
        while_pop(&mut register, &mut popped);
        assert_eq!(popped, DEPTH, "the stack never outgrows its depth");
    }

    // the house bans while loops; a bounded for stands in
    fn while_pop(register: &mut Register, popped: &mut usize) {
        for _ in 0..(DEPTH * 2) {
            if register.pop().is_none() {
                return;
            }
            *popped += 1;
        }
    }
}
