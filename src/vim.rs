//! The modal keymap between the widget and the editor — the slot
//! `editor.rs` names (plan.md § Editor): the sink forwards keys here first,
//! and the grammar answers with editor intents, a swallow, or a pass back
//! to the phase-0 keymap. Pure over the active block's source, so every
//! rung and entry tests headlessly
//! (adr/2026-08-escape-ladder-editor-wide-mode.md).

use dioxus::html::{Key, Modifiers};

use crate::caret;

/// Which grammar the keys speak. The editor opens writing — this app opens
/// on today's note to write in it — so insert is the birth mode and Escape
/// is how the editor starts thinking
/// (adr/2026-08-escape-ladder-editor-wide-mode.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Normal,
    #[default]
    Insert,
}

/// The editor-wide modal state, one signal beside the editor's: boundary
/// slides and fresh activations keep the mode you were in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vim {
    pub mode: Mode,
}

/// One editor intent the grammar decided; the widget's executor applies
/// them in order and never thinks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Collapse the caret to this block-relative byte.
    Place(usize),
    /// Insert text at the caret — o and O open their line with this.
    Type(String),
    /// The ladder's second rung: the block renders again.
    Deactivate,
}

/// What the grammar decided about one keystroke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not the grammar's key: the phase-0 keymap and the chords behave
    /// exactly as they always did.
    Pass,
    /// The grammar consumed it: run these, stop the bubble.
    Acts(Vec<Act>),
    /// Unbound in normal mode: consumed, inert — never inserted as text.
    Swallow,
}

impl Vim {
    /// One keystroke against the active block's `source` with the caret at
    /// block-relative `head`. Chords pass in every mode — modal keys are a
    /// grammar, not commands, and the palette boundary holds
    /// (adr/2026-08-caret-shape-is-the-mode-indicator.md).
    pub fn handle(
        &mut self,
        key: &Key,
        modifiers: Modifiers,
        source: &str,
        head: usize,
    ) -> Outcome {
        if modifiers.ctrl() || modifiers.meta() {
            return Outcome::Pass;
        }
        match self.mode {
            Mode::Insert => self.insert_key(key, source, head),
            Mode::Normal => self.normal_key(key, modifiers, source, head),
        }
    }

    /// Insert mode is phase 0's writing flow, untouched — only Escape is
    /// the grammar's: back to normal, the caret stepping onto the last
    /// cluster of what was just typed, as vim leaves it.
    fn insert_key(&mut self, key: &Key, source: &str, head: usize) -> Outcome {
        if *key != Key::Escape {
            return Outcome::Pass;
        }
        self.mode = Mode::Normal;
        let start = caret::line_start(source, head);
        let target = if head > start {
            caret::prev_cluster(source, head)
        } else {
            head
        };
        Outcome::Acts(vec![Act::Place(target)])
    }

    /// Normal mode: the insert entries, Escape's next rung, the phase-0
    /// arrows passed through until phase 2's motions replace them — and
    /// everything else inert. AltGr characters carry alt, which is why the
    /// printable swallow must see them too.
    fn normal_key(
        &mut self,
        key: &Key,
        _modifiers: Modifiers,
        source: &str,
        head: usize,
    ) -> Outcome {
        let start = caret::line_start(source, head);
        let end = caret::line_end(source, head);
        match key {
            Key::Escape => Outcome::Acts(vec![Act::Deactivate]),
            // caret movement stays answerable until the motions arrive
            Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End => Outcome::Pass,
            Key::Character(character) => match character.as_str() {
                "i" => self.enter_insert(vec![]),
                // append: after the cluster under the caret, never past
                // the line
                "a" => {
                    let target = if head < end {
                        caret::next_cluster(source, head)
                    } else {
                        head
                    };
                    self.enter_insert(vec![Act::Place(target)])
                }
                "I" => self.enter_insert(vec![Act::Place(first_non_blank(
                    source, start, end,
                ))]),
                "A" => self.enter_insert(vec![Act::Place(end)]),
                // open a line below: a newline at the line's end, the
                // caret riding past it onto the fresh line
                "o" => self.enter_insert(vec![
                    Act::Place(end),
                    Act::Type("\n".to_string()),
                ]),
                // open a line above: a newline at the line's start, the
                // caret stepping back onto the fresh line before it
                "O" => self.enter_insert(vec![
                    Act::Place(start),
                    Act::Type("\n".to_string()),
                    Act::Place(start),
                ]),
                _ => Outcome::Swallow,
            },
            _ => Outcome::Swallow,
        }
    }

    fn enter_insert(&mut self, acts: Vec<Act>) -> Outcome {
        self.mode = Mode::Insert;
        Outcome::Acts(acts)
    }
}

/// Where `I` lands: the line's first non-blank cluster, or its end when
/// the line is all blank.
fn first_non_blank(source: &str, start: usize, end: usize) -> usize {
    source
        .get(start..end)
        .and_then(|line| {
            line.char_indices()
                .find(|(_, ch)| !ch.is_whitespace())
                .map(|(offset, _)| start + offset)
        })
        .unwrap_or(end)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn character(letter: &str) -> Key {
        Key::Character(letter.to_string())
    }

    fn normal() -> Vim {
        Vim { mode: Mode::Normal }
    }

    #[test]
    fn chords_pass_in_both_modes() {
        let source = "l'été\n";
        for mode in [Mode::Normal, Mode::Insert] {
            let mut vim = Vim { mode };
            for modifiers in [Modifiers::CONTROL, Modifiers::META] {
                assert_eq!(
                    vim.handle(&character("i"), modifiers, source, 0),
                    Outcome::Pass,
                    "{mode:?} {modifiers:?}"
                );
                assert_eq!(vim.mode, mode, "and the mode held");
            }
        }
    }

    #[test]
    fn insert_mode_owns_only_escape() {
        let source = "l'été\n";
        let mut vim = Vim::default();
        assert_eq!(vim.mode, Mode::Insert, "the editor opens writing");
        for key in [
            character("x"),
            character("é"),
            Key::Enter,
            Key::Backspace,
            Key::ArrowLeft,
            Key::Tab,
        ] {
            assert_eq!(
                vim.handle(&key, Modifiers::empty(), source, 3),
                Outcome::Pass,
                "{key:?}"
            );
            assert_eq!(vim.mode, Mode::Insert);
        }
    }

    #[test]
    fn escape_steps_back_onto_the_last_typed_cluster() {
        // "l'été\n" is l(0) '(1) é(2..4) t(4) é(5..7) \n(7); the caret
        // after typing sits at 7 and escape lands on the last é
        let source = "l'été\n";
        let mut vim = Vim::default();
        let outcome = vim.handle(&Key::Escape, Modifiers::empty(), source, 7);
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(5)]));

        // at a line's start there is nothing to step back onto
        let mut vim = Vim::default();
        let outcome = vim.handle(&Key::Escape, Modifiers::empty(), source, 0);
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(0)]));

        // on the empty last line the caret stays put too
        let mut vim = Vim::default();
        let outcome = vim.handle(&Key::Escape, Modifiers::empty(), source, 8);
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(8)]));
    }

    #[test]
    fn the_insert_entries_place_the_caret_where_vim_would() {
        // "  un été" is two blanks, u(2) n(3) blank(4) é(5..7) t(7)
        // é(8..10) \n(10) — the caret stands on the first é
        let source = "  un été\ndeux";
        let head = 5;
        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("i"), Modifiers::empty(), source, head),
            Outcome::Acts(vec![]),
            "i writes where the caret stands"
        );
        assert_eq!(vim.mode, Mode::Insert);

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("a"), Modifiers::empty(), source, head),
            Outcome::Acts(vec![Act::Place(7)]),
            "a appends after the cluster"
        );

        // a at the line's end appends there, never past it
        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("a"), Modifiers::empty(), source, 10),
            Outcome::Acts(vec![Act::Place(10)]),
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("I"), Modifiers::empty(), source, head),
            Outcome::Acts(vec![Act::Place(2)]),
            "I lands on the first non-blank"
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("A"), Modifiers::empty(), source, head),
            Outcome::Acts(vec![Act::Place(10)]),
            "A lands at the line's end"
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("o"), Modifiers::empty(), source, head),
            Outcome::Acts(vec![Act::Place(10), Act::Type("\n".to_string())]),
            "o opens below"
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("O"), Modifiers::empty(), source, head),
            Outcome::Acts(vec![
                Act::Place(0),
                Act::Type("\n".to_string()),
                Act::Place(0),
            ]),
            "O opens above"
        );
        assert_eq!(vim.mode, Mode::Insert, "every entry ends in insert");
    }

    #[test]
    fn i_on_an_all_blank_line_lands_at_its_end() {
        let source = "   \nx";
        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("I"), Modifiers::empty(), source, 1),
            Outcome::Acts(vec![Act::Place(3)]),
        );
    }

    #[test]
    fn escape_in_normal_mode_renders_the_block() {
        let mut vim = normal();
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::empty(), "un\n", 0),
            Outcome::Acts(vec![Act::Deactivate]),
            "the ladder's second rung"
        );
        assert_eq!(vim.mode, Mode::Normal, "the mode survives the rung");
    }

    #[test]
    fn unbound_normal_keys_are_inert() {
        let source = "un\n";
        for key in [
            character("x"),
            character("é"),
            character("Z"),
            Key::Enter,
            Key::Backspace,
            Key::Delete,
            Key::Tab,
        ] {
            let mut vim = normal();
            assert_eq!(
                vim.handle(&key, Modifiers::empty(), source, 0),
                Outcome::Swallow,
                "{key:?}"
            );
            assert_eq!(vim.mode, Mode::Normal);
        }
        // an AltGr character carries alt and must still be inert
        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("€"), Modifiers::ALT, source, 0),
            Outcome::Swallow,
        );
    }

    #[test]
    fn the_arrows_still_answer_in_normal_mode() {
        // phase 2's motions will replace them; until then movement stays
        for key in [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Home,
            Key::End,
        ] {
            let mut vim = normal();
            assert_eq!(
                vim.handle(&key, Modifiers::empty(), "un\n", 0),
                Outcome::Pass,
                "{key:?}"
            );
        }
    }
}
