//! The widget's keystroke translation: what one key means to the active
//! block, decided without touching it. This is the seam the v2 modal keymap
//! grows in — phase 1 slots `Mode` here, between the widget forwarding keys
//! and the `Editor` applying them (`editor.rs`,
//! adr/2026-08-hidden-ime-sink.md).

use dioxus::html::{Key, Modifiers};

use crate::caret;

/// What one keystroke does to the active block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A printable key, inserted at the caret (replacing any selection).
    Insert(String),
    /// Enter: a newline inside the block — a blank line splits it at the
    /// next resegmentation, the existing merge/split semantics.
    NewLine,
    Backspace,
    Delete,
    /// Ctrl+Backspace, the textarea's word erase kept.
    WordBackspace,
    Move {
        motion: caret::Move,
        select: bool,
    },
    /// Ctrl+A, scoped to the block like the textarea it replaced.
    SelectAll,
    Copy,
    Cut,
    Paste,
    /// Consumed with no effect — the Tab chords the grammar passes on
    /// (Ctrl+Tab), whose browser default would walk focus out of the
    /// invisible sink. Plain Tab is the grammar's own indent key
    /// (adr/2026-08-tab-indents-in-every-mode.md).
    Ignore,
}

/// The one translation: `Some` means the widget owns the key (prevent the
/// default, stop the bubble, apply); `None` means the key is not the
/// widget's and bubbles exactly as the textarea let it — Escape to the
/// pane, every app chord (Ctrl+P/L/N/T/Q/1/2, Ctrl+Enter, Ctrl+Shift+V) to
/// its handler.
pub fn action(key: &Key, modifiers: Modifiers) -> Option<Action> {
    let ctrl = modifiers.ctrl();
    let meta = modifiers.meta();
    let shift = modifiers.shift();
    let select = shift;
    match key {
        Key::Character(character) if ctrl || meta => {
            match character.as_str() {
                // Ctrl+Shift+V stays the capture chord's; shifted copies
                // of the rest are nobody's and bubble inert
                "c" if !shift => Some(Action::Copy),
                "x" if !shift => Some(Action::Cut),
                "v" if !shift => Some(Action::Paste),
                "a" if !shift => Some(Action::SelectAll),
                _ => None,
            }
        }
        // alt stays insertable: AltGr characters on a French layout can
        // carry it, and swallowing them would eat « » € œ
        Key::Character(character) => {
            Some(Action::Insert(character.to_string()))
        }
        Key::Enter if !ctrl => Some(Action::NewLine),
        Key::Backspace if ctrl => Some(Action::WordBackspace),
        Key::Backspace => Some(Action::Backspace),
        Key::Delete => Some(Action::Delete),
        Key::ArrowLeft => Some(Action::Move {
            motion: if ctrl {
                caret::Move::WordLeft
            } else {
                caret::Move::Left
            },
            select,
        }),
        Key::ArrowRight => Some(Action::Move {
            motion: if ctrl {
                caret::Move::WordRight
            } else {
                caret::Move::Right
            },
            select,
        }),
        Key::ArrowUp if !ctrl => Some(Action::Move {
            motion: caret::Move::Up,
            select,
        }),
        Key::ArrowDown if !ctrl => Some(Action::Move {
            motion: caret::Move::Down,
            select,
        }),
        Key::Home if !ctrl => Some(Action::Move {
            motion: caret::Move::LineStart,
            select,
        }),
        Key::End if !ctrl => Some(Action::Move {
            motion: caret::Move::LineEnd,
            select,
        }),
        Key::Tab => Some(Action::Ignore),
        _ => None,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn character(letter: &str) -> Key {
        Key::Character(letter.to_string())
    }

    #[test]
    fn printable_keys_insert_whatever_they_carry() {
        for text in ["a", "é", "«", " "] {
            assert_eq!(
                action(&character(text), Modifiers::empty()),
                Some(Action::Insert(text.to_string())),
            );
        }
        // shift means an uppercase character, alt an AltGr one — both type
        assert_eq!(
            action(&character("É"), Modifiers::SHIFT),
            Some(Action::Insert("É".to_string())),
        );
        assert_eq!(
            action(&character("€"), Modifiers::ALT),
            Some(Action::Insert("€".to_string())),
        );
    }

    #[test]
    fn the_edit_keys_answer() {
        assert_eq!(
            action(&Key::Enter, Modifiers::empty()),
            Some(Action::NewLine)
        );
        assert_eq!(
            action(&Key::Backspace, Modifiers::empty()),
            Some(Action::Backspace)
        );
        assert_eq!(
            action(&Key::Backspace, Modifiers::CONTROL),
            Some(Action::WordBackspace)
        );
        assert_eq!(
            action(&Key::Delete, Modifiers::empty()),
            Some(Action::Delete)
        );
        assert_eq!(
            action(&Key::Tab, Modifiers::empty()),
            Some(Action::Ignore)
        );
    }

    #[test]
    fn arrows_move_and_shift_selects() {
        let cases = [
            (Key::ArrowLeft, caret::Move::Left),
            (Key::ArrowRight, caret::Move::Right),
            (Key::ArrowUp, caret::Move::Up),
            (Key::ArrowDown, caret::Move::Down),
            (Key::Home, caret::Move::LineStart),
            (Key::End, caret::Move::LineEnd),
        ];
        for (key, motion) in cases {
            assert_eq!(
                action(&key, Modifiers::empty()),
                Some(Action::Move {
                    motion,
                    select: false
                }),
            );
            assert_eq!(
                action(&key, Modifiers::SHIFT),
                Some(Action::Move {
                    motion,
                    select: true
                }),
            );
        }
    }

    #[test]
    fn ctrl_arrows_step_by_word() {
        assert_eq!(
            action(&Key::ArrowLeft, Modifiers::CONTROL),
            Some(Action::Move {
                motion: caret::Move::WordLeft,
                select: false
            }),
        );
        assert_eq!(
            action(&Key::ArrowRight, Modifiers::CONTROL | Modifiers::SHIFT),
            Some(Action::Move {
                motion: caret::Move::WordRight,
                select: true
            }),
        );
    }

    #[test]
    fn the_clipboard_chords_answer_unshifted_only() {
        assert_eq!(
            action(&character("c"), Modifiers::CONTROL),
            Some(Action::Copy)
        );
        assert_eq!(
            action(&character("x"), Modifiers::CONTROL),
            Some(Action::Cut)
        );
        assert_eq!(
            action(&character("v"), Modifiers::CONTROL),
            Some(Action::Paste)
        );
        assert_eq!(
            action(&character("a"), Modifiers::CONTROL),
            Some(Action::SelectAll)
        );
        // Ctrl+Shift+V is the capture chord's and must bubble
        assert_eq!(
            action(&character("v"), Modifiers::CONTROL | Modifiers::SHIFT),
            None
        );
        assert_eq!(
            action(&character("c"), Modifiers::CONTROL | Modifiers::SHIFT),
            None
        );
    }

    #[test]
    fn everything_else_bubbles_as_it_always_did() {
        // the app chords, Escape, and the keys nothing owns
        for (key, modifiers) in [
            (Key::Escape, Modifiers::empty()),
            (character("p"), Modifiers::CONTROL),
            (character("l"), Modifiers::CONTROL),
            (character("n"), Modifiers::CONTROL),
            (character("t"), Modifiers::CONTROL),
            (character("q"), Modifiers::CONTROL),
            (character("1"), Modifiers::CONTROL),
            (character("2"), Modifiers::CONTROL),
            (Key::Enter, Modifiers::CONTROL),
            (Key::ArrowUp, Modifiers::CONTROL),
            (Key::ArrowDown, Modifiers::CONTROL),
            (Key::Home, Modifiers::CONTROL),
            (Key::End, Modifiers::CONTROL),
            (Key::F5, Modifiers::empty()),
            (Key::Dead, Modifiers::empty()),
        ] {
            assert_eq!(action(&key, modifiers), None, "{key:?}");
        }
    }
}
