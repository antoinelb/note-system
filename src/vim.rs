//! The modal keymap between the widget and the editor — the slot
//! `editor.rs` names (plan.md § Editor): the sink forwards keys here first,
//! and the grammar answers with editor intents, a swallow, or a pass back
//! to the phase-0 keymap. Pure over a snapshot of the note, so every rung,
//! entry and motion tests headlessly
//! (adr/2026-08-escape-ladder-editor-wide-mode.md).

use dioxus::html::{Key, Modifiers};

use crate::blocks::Block;
use crate::caret;
use crate::motions::{self, FindKind, Lines, Motion};

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

/// What one keystroke sees: the note's text, its block map and the caret's
/// note-global head — read-only, so `handle` stays pure over it.
pub struct View<'a> {
    pub text: &'a str,
    pub blocks: &'a [Block],
    pub head: usize,
}

/// The editor-wide modal state, one signal beside the editor's: boundary
/// slides and fresh activations keep the mode — and the pending grammar —
/// you were in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vim {
    pub mode: Mode,
    /// Digits swallowed toward the next motion; 0 means none.
    count: u32,
    /// A key that awaits exactly one more key: g, or f F t T.
    prefix: Option<Prefix>,
    /// The column a run of j and k holds through short lines, in clusters.
    goal: Option<usize>,
    /// What ; repeats and , reverses.
    last_find: Option<(FindKind, char)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefix {
    Go,
    Find(FindKind),
}

/// One editor intent the grammar decided; the widget's executor applies
/// them in order and never thinks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Collapse the caret to this note-global byte, waking the block that
    /// owns it.
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
    /// Consumed with nothing to run — an unbound key, a failed motion, a
    /// prefix or digit still accumulating.
    Swallow,
}

impl Vim {
    /// One keystroke against the note. Chords pass in every mode — modal
    /// keys are a grammar, not commands, and the palette boundary holds
    /// (adr/2026-08-caret-shape-is-the-mode-indicator.md).
    pub fn handle(
        &mut self,
        key: &Key,
        modifiers: Modifiers,
        view: &View,
    ) -> Outcome {
        if modifiers.ctrl() || modifiers.meta() {
            return Outcome::Pass;
        }
        match self.mode {
            Mode::Insert => self.insert_key(key, view),
            Mode::Normal => self.normal_key(key, view),
        }
    }

    /// Insert mode is phase 0's writing flow, untouched — only Escape is
    /// the grammar's: back to normal, the caret stepping onto the last
    /// cluster of what was just typed, as vim leaves it.
    fn insert_key(&mut self, key: &Key, view: &View) -> Outcome {
        if *key != Key::Escape {
            return Outcome::Pass;
        }
        self.mode = Mode::Normal;
        let line = Lines::of(view.text, view.blocks).around(view.head);
        let target = if view.head > line.start {
            caret::prev_cluster(view.text, view.head).max(line.start)
        } else {
            view.head
        };
        Outcome::Acts(vec![Act::Place(target)])
    }

    /// Normal mode: counts and prefixes accumulate, motions and insert
    /// entries resolve, Escape climbs its ladder — and everything unbound
    /// is inert. AltGr characters carry alt, which is why the printable
    /// swallow must see them too.
    fn normal_key(&mut self, key: &Key, view: &View) -> Outcome {
        if let Some(prefix) = self.prefix.take() {
            return self.finish_prefix(prefix, key, view);
        }
        match key {
            // the ladder's pending rung: an accumulated count dies first
            Key::Escape if self.count > 0 => {
                self.reset();
                Outcome::Swallow
            }
            Key::Escape => Outcome::Acts(vec![Act::Deactivate]),
            // the phase-0 arrows still answer; a count does not apply to
            // them, so it resets rather than leaking onto the next motion
            Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End => {
                self.reset();
                Outcome::Pass
            }
            Key::Character(character) => {
                self.normal_character(character, view)
            }
            _ => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    fn normal_character(&mut self, character: &str, view: &View) -> Outcome {
        let lines = Lines::of(view.text, view.blocks);
        let line = lines.around(view.head);
        match character {
            digit if is_count_digit(digit, self.count) => {
                let value =
                    digit.chars().next().and_then(|ch| ch.to_digit(10));
                self.count = self
                    .count
                    .saturating_mul(10)
                    .saturating_add(value.unwrap_or(0));
                Outcome::Swallow
            }
            "g" => {
                self.prefix = Some(Prefix::Go);
                Outcome::Swallow
            }
            "f" => self.await_find(FindKind::ForwardOn),
            "F" => self.await_find(FindKind::BackwardOn),
            "t" => self.await_find(FindKind::ForwardBefore),
            "T" => self.await_find(FindKind::BackwardBefore),
            "h" => self.run_motion(Motion::Left, view),
            "l" => self.run_motion(Motion::Right, view),
            "j" => self.run_motion(Motion::Down, view),
            "k" => self.run_motion(Motion::Up, view),
            "w" => self.run_motion(Motion::WordForward, view),
            "b" => self.run_motion(Motion::WordBack, view),
            "e" => self.run_motion(Motion::WordEnd, view),
            "0" => self.run_motion(Motion::LineStart, view),
            "^" => self.run_motion(Motion::FirstNonBlank, view),
            "$" => self.run_motion(Motion::LineEnd, view),
            "G" => self.run_motion(Motion::LastLine, view),
            ";" => self.run_motion(Motion::RepeatFind, view),
            "," => self.run_motion(Motion::RepeatFindBack, view),
            "i" => self.enter_insert(vec![]),
            // append: after the cluster under the caret, never past the
            // line's end
            "a" => {
                let target =
                    caret::next_cluster(view.text, view.head).min(line.end);
                self.enter_insert(vec![Act::Place(target)])
            }
            "I" => self.enter_insert(vec![Act::Place(
                motions::first_non_blank(view.text, &line),
            )]),
            "A" => self.enter_insert(vec![Act::Place(line.end)]),
            // open a line below: a newline at the line's end, the caret
            // riding past it onto the fresh line
            "o" => self.enter_insert(vec![
                Act::Place(line.end),
                Act::Type("\n".to_string()),
            ]),
            // open a line above: a newline at the line's start, the caret
            // stepping back onto the fresh line before it
            "O" => self.enter_insert(vec![
                Act::Place(line.start),
                Act::Type("\n".to_string()),
                Act::Place(line.start),
            ]),
            _ => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    /// The one more key a prefix was waiting for — anything that fits
    /// nothing kills the pending state instantly, and no timer exists.
    fn finish_prefix(
        &mut self,
        prefix: Prefix,
        key: &Key,
        view: &View,
    ) -> Outcome {
        match (prefix, key) {
            (Prefix::Go, Key::Character(character)) if character == "g" => {
                self.run_motion(Motion::FirstLine, view)
            }
            (Prefix::Find(kind), Key::Character(character)) => {
                match character.chars().next() {
                    Some(wanted) => {
                        self.last_find = Some((kind, wanted));
                        self.run_motion(Motion::Find(kind, wanted), view)
                    }
                    None => {
                        self.reset();
                        Outcome::Swallow
                    }
                }
            }
            _ => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    fn await_find(&mut self, kind: FindKind) -> Outcome {
        self.prefix = Some(Prefix::Find(kind));
        Outcome::Swallow
    }

    /// One motion, resolved: the count folds in, the goal column survives
    /// exactly the vertical runs, and a failed motion consumes its key.
    fn run_motion(&mut self, motion: Motion, view: &View) -> Outcome {
        let lines = Lines::of(view.text, view.blocks);
        let count = self.count.max(1) as usize;
        let landed = motions::motion(
            view.text,
            &lines,
            view.head,
            motion,
            count,
            self.goal,
            self.last_find,
        );
        self.count = 0;
        match landed {
            Some((target, goal)) => {
                self.goal = goal;
                Outcome::Acts(vec![Act::Place(target)])
            }
            None => {
                self.goal = None;
                Outcome::Swallow
            }
        }
    }

    fn enter_insert(&mut self, acts: Vec<Act>) -> Outcome {
        self.mode = Mode::Insert;
        self.reset();
        Outcome::Acts(acts)
    }

    fn reset(&mut self) {
        self.count = 0;
        self.prefix = None;
        self.goal = None;
    }
}

/// A digit joins the count when it is 1–9, or 0 with digits already down —
/// a bare 0 is the line-start motion.
fn is_count_digit(character: &str, count: u32) -> bool {
    match character {
        "0" => count > 0,
        _ => {
            character.len() == 1
                && character.chars().all(|ch| ch.is_ascii_digit())
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::blocks;

    /// The motions fixture: heading, two-line list, prose, empty last line.
    const NOTE: &str =
        "= l'été\n\n- une idée\n- deux cafés\n\nLa pluie, enfin arrivée.\n";

    fn character(letter: &str) -> Key {
        Key::Character(letter.to_string())
    }

    fn normal() -> Vim {
        Vim {
            mode: Mode::Normal,
            ..Vim::default()
        }
    }

    /// Feeds a string of keys and answers the last outcome — the grammar
    /// harness: "3j" is a count then a motion.
    fn feed(vim: &mut Vim, keys: &str, view: &View) -> Outcome {
        let mut last = Outcome::Swallow;
        for cluster in
            unicode_segmentation::UnicodeSegmentation::graphemes(keys, true)
        {
            last = vim.handle(&character(cluster), Modifiers::empty(), view);
        }
        last
    }

    fn view<'a>(text: &'a str, blocks: &'a [Block], head: usize) -> View<'a> {
        View { text, blocks, head }
    }

    #[test]
    fn chords_pass_in_both_modes() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        for mode in [Mode::Normal, Mode::Insert] {
            let mut vim = Vim {
                mode,
                ..Vim::default()
            };
            for modifiers in [Modifiers::CONTROL, Modifiers::META] {
                assert_eq!(
                    vim.handle(&character("i"), modifiers, &sight),
                    Outcome::Pass,
                    "{mode:?} {modifiers:?}"
                );
                assert_eq!(vim.mode, mode, "and the mode held");
            }
        }
    }

    #[test]
    fn insert_mode_owns_only_escape() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 3);
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
                vim.handle(&key, Modifiers::empty(), &sight),
                Outcome::Pass,
                "{key:?}"
            );
            assert_eq!(vim.mode, Mode::Insert);
        }
    }

    #[test]
    fn escape_steps_back_onto_the_last_typed_cluster() {
        // the caret after typing "= l'été" sits at 9; escape lands on é
        let parsed = blocks::segment(NOTE);
        let mut vim = Vim::default();
        let outcome = vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(NOTE, &parsed, 9),
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(7)]));

        // at a line's start there is nothing to step back onto
        let mut vim = Vim::default();
        let outcome = vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(NOTE, &parsed, 11),
        );
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(11)]));
    }

    #[test]
    fn the_insert_entries_place_the_caret_where_vim_would() {
        // on the é of "cafés": line "- deux cafés" spans 23..36
        let parsed = blocks::segment(NOTE);
        let head = 33;
        let sight = view(NOTE, &parsed, head);

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("i"), Modifiers::empty(), &sight),
            Outcome::Acts(vec![]),
            "i writes where the caret stands"
        );
        assert_eq!(vim.mode, Mode::Insert);

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("a"), Modifiers::empty(), &sight),
            Outcome::Acts(vec![Act::Place(35)]),
            "a appends after the é"
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("I"), Modifiers::empty(), &sight),
            Outcome::Acts(vec![Act::Place(23)]),
            "I lands on the first non-blank"
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("A"), Modifiers::empty(), &sight),
            Outcome::Acts(vec![Act::Place(36)]),
            "A lands at the line's end"
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("o"), Modifiers::empty(), &sight),
            Outcome::Acts(vec![Act::Place(36), Act::Type("\n".to_string())]),
            "o opens below"
        );

        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("O"), Modifiers::empty(), &sight),
            Outcome::Acts(vec![
                Act::Place(23),
                Act::Type("\n".to_string()),
                Act::Place(23),
            ]),
            "O opens above"
        );
        assert_eq!(vim.mode, Mode::Insert, "every entry ends in insert");

        // a at a line's end appends there, never past it
        let mut vim = normal();
        assert_eq!(
            vim.handle(
                &character("a"),
                Modifiers::empty(),
                &view(NOTE, &parsed, 35),
            ),
            Outcome::Acts(vec![Act::Place(36)]),
        );
    }

    #[test]
    fn escape_in_normal_mode_renders_the_block() {
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        assert_eq!(
            vim.handle(
                &Key::Escape,
                Modifiers::empty(),
                &view(NOTE, &parsed, 0)
            ),
            Outcome::Acts(vec![Act::Deactivate]),
            "the ladder's second rung"
        );
        assert_eq!(vim.mode, Mode::Normal, "the mode survives the rung");
    }

    #[test]
    fn unbound_normal_keys_are_inert() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
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
                vim.handle(&key, Modifiers::empty(), &sight),
                Outcome::Swallow,
                "{key:?}"
            );
            assert_eq!(vim.mode, Mode::Normal);
        }
        // an AltGr character carries alt and must still be inert
        let mut vim = normal();
        assert_eq!(
            vim.handle(&character("€"), Modifiers::ALT, &sight),
            Outcome::Swallow,
        );
    }

    #[test]
    fn the_arrows_still_answer_in_normal_mode() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
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
                vim.handle(&key, Modifiers::empty(), &sight),
                Outcome::Pass,
                "{key:?}"
            );
        }
        // a stale count dies on the way through rather than leaking
        let mut vim = normal();
        feed(&mut vim, "3", &sight);
        vim.handle(&Key::ArrowDown, Modifiers::empty(), &sight);
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::Place(11)]),
            "j after the arrow moves one line, not three"
        );
    }

    // -- phase 2: motions through the grammar --------------------------------

    #[test]
    fn motions_move_and_counts_compose() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::Place(11)]),
            "j crosses into the list block"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "3j", &sight),
            Outcome::Acts(vec![Act::Place(38)]),
            "3j lands on the prose"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "12l", &sight),
            Outcome::Acts(vec![Act::Place(7)]),
            "12l clamps onto the heading's last cluster"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "w", &sight),
            Outcome::Acts(vec![Act::Place(2)]),
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "G", &sight),
            Outcome::Acts(vec![Act::Place(NOTE.len())]),
            "G lands on the real empty last line"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "gg", &view(NOTE, &parsed, 40)),
            Outcome::Acts(vec![Act::Place(0)]),
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "2gg", &sight),
            Outcome::Acts(vec![Act::Place(11)]),
            "[count]gg goes to the line"
        );
    }

    #[test]
    fn the_goal_column_survives_j_runs_and_nothing_else() {
        let parsed = blocks::segment(NOTE);
        // from the é of cafés (33), k clamps onto idée's final e, k again
        // restores column 10 on the heading — wait, the heading is short
        // too: it clamps to é at 7
        let mut vim = normal();
        let Outcome::Acts(first) =
            feed(&mut vim, "k", &view(NOTE, &parsed, 33))
        else {
            panic!("k moves")
        };
        assert_eq!(first, vec![Act::Place(21)]);
        let Outcome::Acts(second) =
            feed(&mut vim, "k", &view(NOTE, &parsed, 21))
        else {
            panic!("k moves")
        };
        assert_eq!(second, vec![Act::Place(7)], "the goal column held");
        // a horizontal motion forgets it
        feed(&mut vim, "h", &view(NOTE, &parsed, 7));
        let Outcome::Acts(third) =
            feed(&mut vim, "j", &view(NOTE, &parsed, 6))
        else {
            panic!("j moves")
        };
        assert_ne!(third, vec![Act::Place(21)], "the goal was forgotten");
    }

    #[test]
    fn find_prefixes_collect_their_character_and_repeat() {
        let text = "un café, un café noir\n";
        let parsed = blocks::segment(text);
        let sight = view(text, &parsed, 0);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "fc", &sight),
            Outcome::Acts(vec![Act::Place(3)]),
        );
        assert_eq!(
            feed(&mut vim, ";", &view(text, &parsed, 3)),
            Outcome::Acts(vec![Act::Place(13)]),
            "; repeats the find"
        );
        assert_eq!(
            feed(&mut vim, ",", &view(text, &parsed, 13)),
            Outcome::Acts(vec![Act::Place(3)]),
            ", reverses it"
        );
        assert_eq!(
            feed(&mut vim, "tz", &sight),
            Outcome::Swallow,
            "a find with no target consumes its keys and stays"
        );
        assert_eq!(
            feed(&mut vim, "Fc", &view(text, &parsed, 13)),
            Outcome::Acts(vec![Act::Place(3)]),
            "F looks back"
        );
        assert_eq!(
            feed(&mut vim, "Tc", &view(text, &parsed, 13)),
            Outcome::Acts(vec![Act::Place(4)]),
            "T stops just after"
        );
        // 2fc lands on the second c
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "2fc", &sight),
            Outcome::Acts(vec![Act::Place(13)]),
        );
    }

    #[test]
    fn zero_is_a_motion_alone_and_a_digit_after_one() {
        let text = "dix mots pour dix idées\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "0", &view(text, &parsed, 9)),
            Outcome::Acts(vec![Act::Place(0)]),
            "a bare 0 is line start"
        );
        // 10l reads as count ten, not as a 0 motion after a 1
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "10l", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(10)]),
        );
    }

    #[test]
    fn a_broken_prefix_dies_instantly() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        // g then something that is not g
        let mut vim = normal();
        feed(&mut vim, "g", &sight);
        assert_eq!(feed(&mut vim, "z", &sight), Outcome::Swallow);
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::Place(11)]),
            "the grammar recovered"
        );
        // a prefix broken by a non-character key
        let mut vim = normal();
        feed(&mut vim, "f", &sight);
        assert_eq!(
            vim.handle(&Key::Enter, Modifiers::empty(), &sight),
            Outcome::Swallow
        );
        // and by a character key carrying no character at all
        let mut vim = normal();
        feed(&mut vim, "f", &sight);
        assert_eq!(
            vim.handle(
                &Key::Character(String::new()),
                Modifiers::empty(),
                &sight
            ),
            Outcome::Swallow
        );
        // and an accumulated count dies on escape, which then still
        // climbs its ladder
        let mut vim = normal();
        feed(&mut vim, "42", &sight);
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::empty(), &sight),
            Outcome::Swallow,
            "the pending rung"
        );
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::empty(), &sight),
            Outcome::Acts(vec![Act::Deactivate]),
        );
    }

    #[test]
    fn caret_motion_answers_first_non_blank() {
        let text = "   trois mots\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "^", &view(text, &parsed, 12)),
            Outcome::Acts(vec![Act::Place(3)]),
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "$", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(12)]),
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "b", &view(text, &parsed, 12)),
            Outcome::Acts(vec![Act::Place(9)]),
            "b to the word's start"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "e", &view(text, &parsed, 9)),
            Outcome::Acts(vec![Act::Place(12)]),
            "e to the word's end"
        );
    }
}
