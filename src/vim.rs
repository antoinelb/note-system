//! The modal keymap between the widget and the editor — the slot
//! `editor.rs` names (plan.md § Editor): the sink forwards keys here first,
//! and the grammar answers with editor intents, a swallow, or a pass back
//! to the phase-0 keymap. Pure over a snapshot of the note, so every rung,
//! entry, motion and operator tests headlessly
//! (adr/2026-08-escape-ladder-editor-wide-mode.md).

use std::ops::Range;

use dioxus::html::{Key, Modifiers};

use crate::blocks::Block;
use crate::caret;
use crate::motions::{self, FindKind, Lines, Motion, ObjectKind};

/// Which grammar the keys speak. The editor opens writing — this app opens
/// on today's note to write in it — so insert is the birth mode and Escape
/// is how the editor starts thinking
/// (adr/2026-08-escape-ladder-editor-wide-mode.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Normal,
    #[default]
    Insert,
    /// See the span before choosing the verb
    /// (adr/2026-08-visual-selection-is-the-anchor.md).
    Visual(VisualKind),
}

/// v extends by clusters, V by whole lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualKind {
    Char,
    Line,
}

/// What one keystroke sees: the note's text, its block map and the caret's
/// note-global head — read-only, so `handle` stays pure over it.
pub struct View<'a> {
    pub text: &'a str,
    pub blocks: &'a [Block],
    pub head: usize,
    /// The selection's far end — phase 0's anchor, which visual mode rides
    /// (adr/2026-08-visual-selection-is-the-anchor.md).
    pub anchor: usize,
}

/// The editor-wide modal state, one signal beside the editor's: boundary
/// slides and fresh activations keep the mode — and the pending grammar —
/// you were in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Vim {
    pub mode: Mode,
    /// Digits swallowed before any operator; 0 means none.
    count: u32,
    /// Digits swallowed after the operator (d2w); the counts multiply.
    count2: u32,
    /// The verb waiting for its noun.
    operator: Option<Operator>,
    /// A key that awaits exactly one more key: g, f F t T, r, or the
    /// object side i/a after an operator.
    prefix: Option<Prefix>,
    /// The column a run of j and k holds through short lines, in clusters.
    goal: Option<usize>,
    /// What ; repeats and , reverses.
    last_find: Option<(FindKind, char)>,
}

/// The three verbs (adr/2026-08-one-register-the-clipboard.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prefix {
    Go,
    Find(FindKind),
    Replace,
    Object { around: bool },
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
    /// The one register is the system clipboard: what d, c, y cut or copy
    /// goes out through the write seam
    /// (adr/2026-08-one-register-the-clipboard.md).
    SetClipboard(String),
    /// One resolved change: `span` becomes `text`, the caret lands at the
    /// post-splice coordinate — routed by the editor, within-block or
    /// across (adr/2026-08-editor-splice-cross-block.md).
    Splice {
        span: Range<usize>,
        text: String,
        caret: usize,
    },
    /// p and P: async by nature — the executor reads the clipboard, then
    /// `paste_spec` decides pure.
    Paste { before: bool, count: usize },
    /// Visual's motion: the head moves, the anchor holds.
    Extend(usize),
    /// o in visual: the caret jumps to the selection's other end.
    SwapEnds,
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

/// How a motion behaves under an operator — vim's real distinction,
/// recorded in adr/2026-08-motions-on-visible-lines.md and encoded once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpanKind {
    Exclusive,
    Inclusive,
    Linewise,
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
            Mode::Visual(kind) => self.visual_key(kind, key, view),
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

    /// Normal mode: counts, verbs and prefixes accumulate; motions,
    /// objects and entries resolve; Escape climbs its ladder — and
    /// everything unbound is inert. AltGr characters carry alt, which is
    /// why the printable swallow must see them too.
    fn normal_key(&mut self, key: &Key, view: &View) -> Outcome {
        if let Some(prefix) = self.prefix.take() {
            return self.finish_prefix(prefix, key, view);
        }
        match key {
            // the ladder's pending rung: accumulated grammar dies first
            Key::Escape if self.pending() => {
                self.reset();
                Outcome::Swallow
            }
            Key::Escape => Outcome::Acts(vec![Act::Deactivate]),
            // the phase-0 arrows still answer; pending grammar does not
            // apply to them, so it resets rather than leaking
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
        let line = Lines::of(view.text, view.blocks).around(view.head);
        match character {
            digit if self.is_count_digit(digit) => {
                let value =
                    digit.chars().next().and_then(|ch| ch.to_digit(10));
                let slot = if self.operator.is_some() {
                    &mut self.count2
                } else {
                    &mut self.count
                };
                *slot =
                    slot.saturating_mul(10).saturating_add(value.unwrap_or(0));
                Outcome::Swallow
            }
            "g" => {
                self.prefix = Some(Prefix::Go);
                Outcome::Swallow
            }
            "f" => self.await_prefix(Prefix::Find(FindKind::ForwardOn)),
            "F" => self.await_prefix(Prefix::Find(FindKind::BackwardOn)),
            "t" => self.await_prefix(Prefix::Find(FindKind::ForwardBefore)),
            "T" => self.await_prefix(Prefix::Find(FindKind::BackwardBefore)),
            "d" => self.operator_key(Operator::Delete, view),
            "c" => self.operator_key(Operator::Change, view),
            "y" => self.operator_key(Operator::Yank, view),
            // the object sides, only meaningful behind a verb — otherwise
            // i and a are the insert entries below
            "i" if self.operator.is_some() => {
                self.await_prefix(Prefix::Object { around: false })
            }
            "a" if self.operator.is_some() => {
                self.await_prefix(Prefix::Object { around: true })
            }
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
            "I" if self.operator.is_none() => {
                self.enter_insert(vec![Act::Place(motions::first_non_blank(
                    view.text, &line,
                ))])
            }
            "A" if self.operator.is_none() => {
                self.enter_insert(vec![Act::Place(line.end)])
            }
            // open a line below: a newline at the line's end, the caret
            // riding past it onto the fresh line
            "o" if self.operator.is_none() => self.enter_insert(vec![
                Act::Place(line.end),
                Act::Type("\n".to_string()),
            ]),
            // open a line above: a newline at the line's start, the caret
            // stepping back onto the fresh line before it
            "O" if self.operator.is_none() => self.enter_insert(vec![
                Act::Place(line.start),
                Act::Type("\n".to_string()),
                Act::Place(line.start),
            ]),
            // see the span before choosing the verb
            "v" if self.operator.is_none() => {
                self.reset();
                self.mode = Mode::Visual(VisualKind::Char);
                Outcome::Acts(vec![])
            }
            "V" if self.operator.is_none() => {
                self.reset();
                self.mode = Mode::Visual(VisualKind::Line);
                Outcome::Acts(vec![])
            }
            // the shorthands, spelled as themselves rather than rewritten:
            // their edge behaviour (x at a line's last cluster) is exact
            "D" if self.operator.is_none() => self.finish_operator(
                Operator::Delete,
                view.head..line.end,
                false,
                view,
            ),
            "C" if self.operator.is_none() => self.finish_operator(
                Operator::Change,
                view.head..line.end,
                false,
                view,
            ),
            "Y" if self.operator.is_none() => {
                self.current_lines(Operator::Yank, view)
            }
            "x" if self.operator.is_none() => {
                self.cut_clusters(view, &line, true)
            }
            "X" if self.operator.is_none() => {
                self.cut_clusters(view, &line, false)
            }
            "r" if self.operator.is_none() => {
                self.await_prefix(Prefix::Replace)
            }
            "~" if self.operator.is_none() => self.toggle_case(view, &line),
            "p" if self.operator.is_none() => self.paste(false),
            "P" if self.operator.is_none() => self.paste(true),
            _ => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    /// Visual mode: motions extend, o swaps the ends, the verbs apply to
    /// the selection, Escape returns to normal with the caret at the head.
    fn visual_key(
        &mut self,
        kind: VisualKind,
        key: &Key,
        view: &View,
    ) -> Outcome {
        if let Some(prefix) = self.prefix.take() {
            return self.finish_prefix(prefix, key, view);
        }
        match key {
            Key::Escape => {
                self.reset();
                self.mode = Mode::Normal;
                Outcome::Acts(vec![Act::Place(view.head)])
            }
            // the arrows extend too — collapsing mid-visual would be the
            // one thing no one means
            Key::ArrowLeft => self.run_motion(Motion::Left, view),
            Key::ArrowRight => self.run_motion(Motion::Right, view),
            Key::ArrowUp => self.run_motion(Motion::Up, view),
            Key::ArrowDown => self.run_motion(Motion::Down, view),
            Key::Home => self.run_motion(Motion::LineStart, view),
            Key::End => self.run_motion(Motion::LineEnd, view),
            Key::Character(character) => {
                self.visual_character(kind, character, view)
            }
            _ => Outcome::Swallow,
        }
    }

    fn visual_character(
        &mut self,
        kind: VisualKind,
        character: &str,
        view: &View,
    ) -> Outcome {
        match character {
            digit if self.is_count_digit(digit) => {
                let value =
                    digit.chars().next().and_then(|ch| ch.to_digit(10));
                self.count = self
                    .count
                    .saturating_mul(10)
                    .saturating_add(value.unwrap_or(0));
                Outcome::Swallow
            }
            // the same kind toggles out; the other switches in place
            "v" => {
                self.mode = if kind == VisualKind::Char {
                    Mode::Normal
                } else {
                    Mode::Visual(VisualKind::Char)
                };
                if self.mode == Mode::Normal {
                    Outcome::Acts(vec![Act::Place(view.head)])
                } else {
                    Outcome::Acts(vec![])
                }
            }
            "V" => {
                self.mode = if kind == VisualKind::Line {
                    Mode::Normal
                } else {
                    Mode::Visual(VisualKind::Line)
                };
                if self.mode == Mode::Normal {
                    Outcome::Acts(vec![Act::Place(view.head)])
                } else {
                    Outcome::Acts(vec![])
                }
            }
            "o" => Outcome::Acts(vec![Act::SwapEnds]),
            "d" | "x" => self.visual_operate(Operator::Delete, kind, view),
            "c" => self.visual_operate(Operator::Change, kind, view),
            "y" => self.visual_operate(Operator::Yank, kind, view),
            "g" => {
                self.prefix = Some(Prefix::Go);
                Outcome::Swallow
            }
            "f" => self.await_prefix(Prefix::Find(FindKind::ForwardOn)),
            "F" => self.await_prefix(Prefix::Find(FindKind::BackwardOn)),
            "t" => self.await_prefix(Prefix::Find(FindKind::ForwardBefore)),
            "T" => self.await_prefix(Prefix::Find(FindKind::BackwardBefore)),
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
            _ => Outcome::Swallow,
        }
    }

    /// A verb over the selection: char-wise takes both end clusters, as
    /// vim's visual does; line-wise takes the whole lines. The mode falls
    /// back to normal — or into insert, when the verb was c.
    fn visual_operate(
        &mut self,
        op: Operator,
        kind: VisualKind,
        view: &View,
    ) -> Outcome {
        let low = view.head.min(view.anchor);
        let high = view.head.max(view.anchor);
        self.mode = Mode::Normal;
        match kind {
            VisualKind::Char => {
                let span = low..caret::next_cluster(view.text, high);
                self.finish_operator(op, span, false, view)
            }
            VisualKind::Line => {
                let lines = Lines::of(view.text, view.blocks);
                let span = motions::linewise_span(
                    &lines,
                    lines.row_of(low),
                    lines.row_of(high),
                );
                self.finish_operator(op, span, true, view)
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
        let Key::Character(character) = key else {
            self.reset();
            return Outcome::Swallow;
        };
        match prefix {
            Prefix::Go if character == "g" => {
                self.run_motion(Motion::FirstLine, view)
            }
            Prefix::Find(kind) => match character.chars().next() {
                Some(wanted) => {
                    self.last_find = Some((kind, wanted));
                    self.run_motion(Motion::Find(kind, wanted), view)
                }
                None => {
                    self.reset();
                    Outcome::Swallow
                }
            },
            Prefix::Replace => self.replace_clusters(character, view),
            Prefix::Object { around } => {
                self.finish_object(character, around, view)
            }
            Prefix::Go => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    fn await_prefix(&mut self, prefix: Prefix) -> Outcome {
        self.prefix = Some(prefix);
        Outcome::Swallow
    }

    /// A verb key: the first arms the operator, the doubled one names the
    /// current lines (dd cc yy), a different one aborts.
    fn operator_key(&mut self, op: Operator, view: &View) -> Outcome {
        match self.operator {
            None => {
                self.operator = Some(op);
                Outcome::Swallow
            }
            Some(pending) if pending == op => self.current_lines(op, view),
            Some(_) => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    /// dd cc yy and Y: the operator over [count] whole lines.
    fn current_lines(&mut self, op: Operator, view: &View) -> Outcome {
        let lines = Lines::of(view.text, view.blocks);
        let total = self.effective_count();
        let first = lines.row_of(view.head);
        let last = (first + total - 1).min(lines.rows() - 1);
        let span = motions::linewise_span(&lines, first, last);
        self.finish_operator(op, span, true, view)
    }

    /// One motion, resolved: plain it moves the caret; behind a verb it
    /// names the span. The count folds in multiplied (2d3w is six words).
    fn run_motion(&mut self, motion: Motion, view: &View) -> Outcome {
        match self.operator {
            Some(op) => self.operator_motion(op, motion, view),
            None => {
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
                        // in visual the anchor holds; everywhere else the
                        // landing collapses
                        let act = if matches!(self.mode, Mode::Visual(_)) {
                            Act::Extend(target)
                        } else {
                            Act::Place(target)
                        };
                        Outcome::Acts(vec![act])
                    }
                    None => {
                        self.goal = None;
                        Outcome::Swallow
                    }
                }
            }
        }
    }

    /// verb + motion: the span between here and the landing, cut by the
    /// motion's kind — with vim's two word quirks adopted as house rules:
    /// cw acts as ce on a word, and dw never crosses the line's end.
    fn operator_motion(
        &mut self,
        op: Operator,
        motion: Motion,
        view: &View,
    ) -> Outcome {
        let lines = Lines::of(view.text, view.blocks);
        let on_word = view
            .text
            .get(view.head..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|ch| !ch.is_whitespace());
        let motion = if op == Operator::Change
            && motion == Motion::WordForward
            && on_word
        {
            Motion::WordEnd
        } else {
            motion
        };
        // ; and , normalize into the find they repeat, so the span kind
        // reads off the motion itself
        let normalized = match motion {
            Motion::RepeatFind => self
                .last_find
                .map(|(kind, wanted)| Motion::Find(kind, wanted)),
            Motion::RepeatFindBack => self.last_find.map(|(kind, wanted)| {
                Motion::Find(motions::reverse(kind), wanted)
            }),
            other => Some(other),
        };
        let Some(motion) = normalized else {
            self.reset();
            return Outcome::Swallow;
        };
        let total = self.effective_count();
        let landed = motions::motion(
            view.text,
            &lines,
            view.head,
            motion,
            total,
            None,
            self.last_find,
        );
        let Some((target, _)) = landed else {
            self.reset();
            return Outcome::Swallow;
        };
        match span_kind(motion) {
            SpanKind::Linewise => {
                let first = lines.row_of(view.head.min(target));
                let last = lines.row_of(view.head.max(target));
                let span = motions::linewise_span(&lines, first, last);
                self.finish_operator(op, span, true, view)
            }
            kind => {
                let low = view.head.min(target);
                let mut high = view.head.max(target);
                if kind == SpanKind::Inclusive {
                    high = caret::next_cluster(view.text, high);
                }
                // dw stops at the line's end rather than eating the break
                if motion == Motion::WordForward {
                    let line = lines.around(view.head);
                    high = high.min(line.end).max(low);
                }
                self.finish_operator(op, low..high, false, view)
            }
        }
    }

    /// verb + object: the noun names its own span.
    fn finish_object(
        &mut self,
        character: &str,
        around: bool,
        view: &View,
    ) -> Outcome {
        let (Some(op), Some(kind)) = (self.operator, object_kind(character))
        else {
            self.reset();
            return Outcome::Swallow;
        };
        match motions::object(view.text, view.blocks, view.head, kind, around)
        {
            Some(span) => {
                let linewise = matches!(kind, ObjectKind::Block);
                self.finish_operator(op, span, linewise, view)
            }
            None => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    /// Every operator application funnels here: the register fills, the
    /// splice lands, the caret follows vim's conventions, and c ends in
    /// insert.
    fn finish_operator(
        &mut self,
        op: Operator,
        span: Range<usize>,
        linewise: bool,
        view: &View,
    ) -> Outcome {
        self.reset();
        let cut = view.text.get(span.clone()).unwrap_or_default();
        let mut yanked = cut.to_string();
        if linewise && !yanked.ends_with('\n') {
            // the clipboard is the one register: the trailing newline is
            // how linewise-ness survives the OS round trip
            // (adr/2026-08-one-register-the-clipboard.md)
            yanked.push('\n');
        }
        let mut acts = Vec::new();
        // a charwise nothing yanks nothing; a linewise nothing is still a
        // line and its newline reaches the register, as vim's dd does
        if !yanked.is_empty() {
            acts.push(Act::SetClipboard(yanked));
        }
        match op {
            Operator::Yank => {
                // a charwise yank lands on the span's start; a linewise
                // one leaves the caret where it stood
                let caret = if linewise { view.head } else { span.start };
                acts.push(Act::Place(caret));
            }
            Operator::Delete => {
                let caret = if linewise {
                    linewise_delete_caret(view.text, &span)
                } else {
                    charwise_delete_caret(
                        view.text,
                        &Lines::of(view.text, view.blocks).around(span.start),
                        &span,
                    )
                };
                acts.push(Act::Splice {
                    span,
                    text: String::new(),
                    caret,
                });
            }
            Operator::Change => {
                self.mode = Mode::Insert;
                // a linewise change keeps its line: the final newline
                // survives so typing refills the same line
                let span = if linewise && cut.ends_with('\n') {
                    span.start..span.end - 1
                } else {
                    span
                };
                let caret = span.start;
                acts.push(Act::Splice {
                    span,
                    text: String::new(),
                    caret,
                });
            }
        }
        Outcome::Acts(acts)
    }

    /// x and X: the clusters beside the caret, cut line-scoped — and into
    /// the register, as vim cuts.
    fn cut_clusters(
        &mut self,
        view: &View,
        line: &Range<usize>,
        forward: bool,
    ) -> Outcome {
        let total = self.count.max(1) as usize;
        self.reset();
        let span = if forward {
            let end = (0..total).fold(view.head, |from, _| {
                caret::next_cluster(view.text, from).min(line.end)
            });
            view.head..end
        } else {
            let start = (0..total).fold(view.head, |from, _| {
                caret::prev_cluster(view.text, from).max(line.start)
            });
            start..view.head
        };
        if span.is_empty() {
            return Outcome::Swallow;
        }
        let cut = view.text.get(span.clone()).unwrap_or_default().to_string();
        let caret = if forward {
            charwise_delete_caret(view.text, line, &span)
        } else {
            span.start
        };
        Outcome::Acts(vec![
            Act::SetClipboard(cut),
            Act::Splice {
                span,
                text: String::new(),
                caret,
            },
        ])
    }

    /// r: [count] clusters become [count] copies of the character; fewer
    /// on the line than the count and the whole thing fails, as vim's does.
    fn replace_clusters(&mut self, character: &str, view: &View) -> Outcome {
        let total = self.count.max(1) as usize;
        self.reset();
        let Some(wanted) = character.chars().next() else {
            return Outcome::Swallow;
        };
        let line = Lines::of(view.text, view.blocks).around(view.head);
        let end = (0..total).fold(view.head, |from, _| {
            caret::next_cluster(view.text, from).min(line.end)
        });
        let clusters = view
            .text
            .get(view.head..end)
            .map(|slice| {
                unicode_segmentation::UnicodeSegmentation::graphemes(
                    slice, true,
                )
                .count()
            })
            .unwrap_or(0);
        if clusters < total {
            return Outcome::Swallow;
        }
        let text: String = (0..total).map(|_| wanted).collect();
        let caret = view.head + (total - 1) * wanted.len_utf8();
        Outcome::Acts(vec![Act::Splice {
            span: view.head..end,
            text,
            caret,
        }])
    }

    /// ~: flip the case of [count] clusters and step past them.
    fn toggle_case(&mut self, view: &View, line: &Range<usize>) -> Outcome {
        let total = self.count.max(1) as usize;
        self.reset();
        let end = (0..total).fold(view.head, |from, _| {
            caret::next_cluster(view.text, from).min(line.end)
        });
        let span = view.head..end;
        if span.is_empty() {
            return Outcome::Swallow;
        }
        let flipped: String = view
            .text
            .get(span.clone())
            .unwrap_or_default()
            .chars()
            .flat_map(|ch| {
                if ch.is_uppercase() {
                    ch.to_lowercase().collect::<Vec<char>>()
                } else {
                    ch.to_uppercase().collect::<Vec<char>>()
                }
            })
            .collect();
        let caret = if end >= line.end {
            caret::prev_cluster(view.text, line.end).max(line.start)
        } else {
            end
        };
        Outcome::Acts(vec![Act::Splice {
            span,
            text: flipped,
            caret,
        }])
    }

    fn paste(&mut self, before: bool) -> Outcome {
        let total = self.count.max(1) as usize;
        self.reset();
        Outcome::Acts(vec![Act::Paste {
            before,
            count: total,
        }])
    }

    fn enter_insert(&mut self, acts: Vec<Act>) -> Outcome {
        self.mode = Mode::Insert;
        self.reset();
        Outcome::Acts(acts)
    }

    fn pending(&self) -> bool {
        self.count > 0 || self.count2 > 0 || self.operator.is_some()
    }

    fn effective_count(&mut self) -> usize {
        let total =
            (self.count.max(1) as usize) * (self.count2.max(1) as usize);
        self.count = 0;
        self.count2 = 0;
        total
    }

    /// A digit joins the active count when it is 1–9, or 0 with digits
    /// already down — a bare 0 is the line-start motion.
    fn is_count_digit(&self, character: &str) -> bool {
        let active = if self.operator.is_some() {
            self.count2
        } else {
            self.count
        };
        match character {
            "0" => active > 0,
            _ => {
                character.len() == 1
                    && character.chars().all(|ch| ch.is_ascii_digit())
            }
        }
    }

    fn reset(&mut self) {
        self.count = 0;
        self.count2 = 0;
        self.operator = None;
        self.prefix = None;
        self.goal = None;
    }
}

/// The kind table adr/2026-08-motions-on-visible-lines.md recorded: e \$
/// and the forward finds are inclusive, the verticals and whole-note jumps
/// linewise, everything else exclusive — ; and , arrive here already
/// normalized into the find they repeat.
fn span_kind(motion: Motion) -> SpanKind {
    match motion {
        Motion::Down | Motion::Up | Motion::FirstLine | Motion::LastLine => {
            SpanKind::Linewise
        }
        Motion::WordEnd | Motion::LineEnd => SpanKind::Inclusive,
        Motion::Find(kind, _) => find_span(kind),
        _ => SpanKind::Exclusive,
    }
}

fn find_span(kind: FindKind) -> SpanKind {
    if matches!(kind, FindKind::ForwardOn | FindKind::ForwardBefore) {
        SpanKind::Inclusive
    } else {
        SpanKind::Exclusive
    }
}

/// After a charwise deletion the caret sits at the span's start — stepped
/// back one cluster when the deletion ran through the line's end, so it
/// still rests on a character.
fn charwise_delete_caret(
    text: &str,
    line: &Range<usize>,
    span: &Range<usize>,
) -> usize {
    if span.end >= line.end && span.start > line.start {
        caret::prev_cluster(text, span.start).max(line.start)
    } else {
        span.start
    }
}

/// After a linewise deletion the caret lands on the first non-blank of
/// the line that slid up into the gap.
fn linewise_delete_caret(text: &str, span: &Range<usize>) -> usize {
    let following = text.get(span.end..).unwrap_or_default();
    span.start + motions::blank_prefix(following)
}

/// The nouns: word, the quote flavours, the bracket pairs — French
/// guillemets included — and the block-as-paragraph.
fn object_kind(character: &str) -> Option<ObjectKind> {
    match character {
        "w" => Some(ObjectKind::Word),
        "\"" => Some(ObjectKind::Quote('"')),
        "'" => Some(ObjectKind::Quote('\'')),
        "`" => Some(ObjectKind::Quote('`')),
        "(" | ")" | "b" => Some(ObjectKind::Pair('(', ')')),
        "[" | "]" => Some(ObjectKind::Pair('[', ']')),
        "{" | "}" | "B" => Some(ObjectKind::Pair('{', '}')),
        "<" | ">" => Some(ObjectKind::Pair('<', '>')),
        "«" | "»" => Some(ObjectKind::Pair('«', '»')),
        "p" => Some(ObjectKind::Block),
        _ => None,
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
    /// harness: "d2w" is a verb, a count, a noun.
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
        // collapsed by default: visual tests build their own anchor
        View {
            text,
            blocks,
            head,
            anchor: head,
        }
    }

    fn spread<'a>(
        text: &'a str,
        blocks: &'a [Block],
        anchor: usize,
        head: usize,
    ) -> View<'a> {
        View {
            text,
            blocks,
            head,
            anchor,
        }
    }

    // -- phases 1 and 2, unchanged -------------------------------------------

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
        let parsed = blocks::segment(NOTE);
        let mut vim = Vim::default();
        let outcome = vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(NOTE, &parsed, 9),
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(7)]));

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
            character("z"),
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
        let mut vim = normal();
        feed(&mut vim, "3", &sight);
        vim.handle(&Key::ArrowDown, Modifiers::empty(), &sight);
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::Place(11)]),
            "j after the arrow moves one line, not three"
        );
    }

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
            feed(&mut vim, "G", &sight),
            Outcome::Acts(vec![Act::Place(NOTE.len())]),
            "G lands on the real empty last line"
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
        let mut vim = normal();
        feed(&mut vim, "g", &sight);
        assert_eq!(feed(&mut vim, "z", &sight), Outcome::Swallow);
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::Place(11)]),
            "the grammar recovered"
        );
        let mut vim = normal();
        feed(&mut vim, "f", &sight);
        assert_eq!(
            vim.handle(&Key::Enter, Modifiers::empty(), &sight),
            Outcome::Swallow
        );
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

    // -- phase 3: operators, objects, register, paste ------------------------

    /// The acts of one fed key string, panicking on anything but Acts.
    fn acts_of(keys: &str, text: &str, head: usize) -> Vec<Act> {
        let parsed = blocks::segment(text);
        let mut vim = normal();
        match feed(&mut vim, keys, &view(text, &parsed, head)) {
            Outcome::Acts(acts) => acts,
            other => panic!("{keys}: expected acts, got {other:?}"),
        }
    }

    #[test]
    fn the_operator_motion_matrix_cuts_the_right_spans() {
        let text = "un café noir\nposé là\n";
        // dw from the start: exclusive, to café's c
        assert_eq!(
            acts_of("dw", text, 0),
            vec![
                Act::SetClipboard("un ".into()),
                Act::Splice {
                    span: 0..3,
                    text: String::new(),
                    caret: 0
                },
            ],
        );
        // de: inclusive, through un's n
        assert_eq!(
            acts_of("de", text, 0),
            vec![
                Act::SetClipboard("un".into()),
                Act::Splice {
                    span: 0..2,
                    text: String::new(),
                    caret: 0
                },
            ],
        );
        // d$: through the line's last cluster, caret stepping back
        assert_eq!(
            acts_of("d$", text, 3),
            vec![
                Act::SetClipboard("café noir".into()),
                Act::Splice {
                    span: 3..13,
                    text: String::new(),
                    caret: 2
                },
            ],
        );
        // dfé: inclusive find over the accent
        assert_eq!(
            acts_of("dfé", text, 0),
            vec![
                Act::SetClipboard("un café".into()),
                Act::Splice {
                    span: 0..8,
                    text: String::new(),
                    caret: 0
                },
            ],
        );
        // dj: linewise, both lines and the newline between
        assert_eq!(
            acts_of("dj", text, 3),
            vec![
                Act::SetClipboard("un café noir\nposé là\n".into()),
                Act::Splice {
                    span: 0..24,
                    text: String::new(),
                    caret: 0
                },
            ],
        );
        // 2dw = d2w: the counts multiply
        assert_eq!(acts_of("2dw", text, 0), acts_of("d2w", text, 0));
    }

    #[test]
    fn dw_never_eats_the_line_break() {
        let text = "fin\nsuite\n";
        assert_eq!(
            acts_of("dw", text, 0),
            vec![
                Act::SetClipboard("fin".into()),
                Act::Splice {
                    span: 0..3,
                    text: String::new(),
                    caret: 0
                },
            ],
            "dw on the last word stops at the line's end"
        );
    }

    #[test]
    fn cw_acts_as_ce_and_c_ends_in_insert() {
        let text = "mot suivant\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "cw", &view(text, &parsed, 0));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("mot".into()),
                Act::Splice {
                    span: 0..3,
                    text: String::new(),
                    caret: 0
                },
            ]),
            "cw took the word, not its trailing blank"
        );
        assert_eq!(vim.mode, Mode::Insert);
    }

    #[test]
    fn doubled_operators_take_whole_lines() {
        let text = "une\ndeux\ntrois\n";
        // dd: the line and its newline
        assert_eq!(
            acts_of("dd", text, 5),
            vec![
                Act::SetClipboard("deux\n".into()),
                Act::Splice {
                    span: 4..9,
                    text: String::new(),
                    caret: 4
                },
            ],
        );
        // 2dd from the top
        assert_eq!(
            acts_of("2dd", text, 0),
            vec![
                Act::SetClipboard("une\ndeux\n".into()),
                Act::Splice {
                    span: 0..9,
                    text: String::new(),
                    caret: 0
                },
            ],
        );
        // cc keeps its line: the span stops before the newline
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "cc", &view(text, &parsed, 5));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("deux\n".into()),
                Act::Splice {
                    span: 4..8,
                    text: String::new(),
                    caret: 4
                },
            ]),
        );
        assert_eq!(vim.mode, Mode::Insert);
        // yy fills the register and stays put
        assert_eq!(
            acts_of("yy", text, 5),
            vec![Act::SetClipboard("deux\n".into()), Act::Place(5)],
        );
    }

    #[test]
    fn dd_on_a_blocks_last_line_leaves_the_separator_to_merge() {
        // the heading line ends at its block's content: no newline to eat
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        let outcome = feed(&mut vim, "dd", &view(NOTE, &parsed, 2));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("= l'été\n".into()),
                Act::Splice {
                    span: 0..9,
                    text: String::new(),
                    caret: 0
                },
            ]),
            "the register still carries the linewise newline"
        );
    }

    #[test]
    fn the_shorthands_answer() {
        let text = "un café\n";
        // D to the end, caret stepping back onto the last survivor
        assert_eq!(
            acts_of("D", text, 3),
            vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 3..8,
                    text: String::new(),
                    caret: 2
                },
            ],
        );
        // C likewise, but writing
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "C", &view(text, &parsed, 3));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 3..8,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        assert_eq!(vim.mode, Mode::Insert);
        // Y is yy
        assert_eq!(
            acts_of("Y", text, 3),
            vec![Act::SetClipboard("un café\n".into()), Act::Place(3)],
        );
        // x cuts the cluster under the caret — the é whole
        assert_eq!(
            acts_of("x", text, 6),
            vec![
                Act::SetClipboard("é".into()),
                Act::Splice {
                    span: 6..8,
                    text: String::new(),
                    caret: 5
                },
            ],
        );
        // X cuts backward
        assert_eq!(
            acts_of("X", text, 5),
            vec![
                Act::SetClipboard("a".into()),
                Act::Splice {
                    span: 4..5,
                    text: String::new(),
                    caret: 4
                },
            ],
        );
        // 2x from the start
        assert_eq!(
            acts_of("2x", text, 0),
            vec![
                Act::SetClipboard("un".into()),
                Act::Splice {
                    span: 0..2,
                    text: String::new(),
                    caret: 0
                },
            ],
        );
        // x on the empty last line has nothing to cut
        assert_eq!(
            {
                let parsed = blocks::segment(text);
                let mut vim = normal();
                feed(&mut vim, "x", &view(text, &parsed, 8))
            },
            Outcome::Swallow,
        );
    }

    #[test]
    fn r_replaces_and_tilde_flips() {
        let text = "été la\n";
        // ré: the cluster becomes the collected character
        assert_eq!(
            acts_of("rx", text, 0),
            vec![Act::Splice {
                span: 0..2,
                text: "x".into(),
                caret: 0
            }],
        );
        // 2ra replaces two clusters with aa
        assert_eq!(
            acts_of("2ra", text, 0),
            vec![Act::Splice {
                span: 0..3,
                text: "aa".into(),
                caret: 1
            }],
        );
        // a count deeper than the line fails whole, as vim's r does
        assert_eq!(
            {
                let parsed = blocks::segment(text);
                let mut vim = normal();
                feed(&mut vim, "9rz", &view(text, &parsed, 0))
            },
            Outcome::Swallow,
        );
        // ~ flips case and steps right
        assert_eq!(
            acts_of("~", text, 0),
            vec![Act::Splice {
                span: 0..2,
                text: "É".into(),
                caret: 2
            }],
        );
        // ~ at the line's last cluster clamps the step
        assert_eq!(
            acts_of("~", text, 7),
            vec![Act::Splice {
                span: 7..8,
                text: "A".into(),
                caret: 7
            }],
        );
    }

    #[test]
    fn objects_name_their_spans() {
        let text = "dit « l'idée (la vraie) compte »\n";
        // diw on idée: the word run only
        assert_eq!(
            acts_of("diw", text, 9),
            vec![
                Act::SetClipboard("idée".into()),
                Act::Splice {
                    span: 9..14,
                    text: String::new(),
                    caret: 9
                },
            ],
        );
        // daw takes the trailing blank too
        assert_eq!(
            acts_of("daw", text, 9),
            vec![
                Act::SetClipboard("idée ".into()),
                Act::Splice {
                    span: 9..15,
                    text: String::new(),
                    caret: 9
                },
            ],
        );
        // di( inside the parentheses, from inside them
        assert_eq!(
            acts_of("di(", text, 17),
            vec![
                Act::SetClipboard("la vraie".into()),
                Act::Splice {
                    span: 16..24,
                    text: String::new(),
                    caret: 16
                },
            ],
        );
        // da« takes the guillemets with it
        let Some(Act::SetClipboard(cut)) =
            acts_of("da«", text, 17).into_iter().next()
        else {
            panic!("da« yanks")
        };
        assert_eq!(cut, "« l'idée (la vraie) compte »");
        // ci" with no quotes on the line fails quietly
        assert_eq!(
            {
                let parsed = blocks::segment(text);
                let mut vim = normal();
                feed(&mut vim, "ci\"", &view(text, &parsed, 9))
            },
            Outcome::Swallow,
        );
    }

    #[test]
    fn quote_objects_pair_left_to_right() {
        let text = "un \"mot\" et \"deux\"\n";
        assert_eq!(
            acts_of("di\"", text, 5),
            vec![
                Act::SetClipboard("mot".into()),
                Act::Splice {
                    span: 4..7,
                    text: String::new(),
                    caret: 4
                },
            ],
        );
        // from between the pairs, the next pair is the object
        assert_eq!(
            acts_of("da\"", text, 9),
            vec![
                Act::SetClipboard("\"deux\"".into()),
                Act::Splice {
                    span: 12..18,
                    text: String::new(),
                    caret: 11
                },
            ],
        );
    }

    #[test]
    fn ip_and_ap_are_the_block() {
        // the list block: content 11..36, range runs through the separator
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        let outcome = feed(&mut vim, "dip", &view(NOTE, &parsed, 25));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("- une idée\n- deux cafés\n".to_string()),
                Act::Splice {
                    span: 11..36,
                    text: String::new(),
                    caret: 11
                },
            ]),
            "ip is the block's content, linewise in the register"
        );
        let mut vim = normal();
        let outcome = feed(&mut vim, "dap", &view(NOTE, &parsed, 25));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("- une idée\n- deux cafés\n\n".to_string()),
                Act::Splice {
                    span: 11..38,
                    text: String::new(),
                    caret: 11
                },
            ]),
            "ap takes the separator with it"
        );
    }

    #[test]
    fn every_object_key_names_its_noun() {
        let text = "un ['x'] et {`y`} puis <où>\n";
        let parsed = blocks::segment(text);
        // each nested side resolves through the grammar; the exact spans
        // are motions' business — here the keys must reach their nouns
        for (keys, at) in [
            ("di'", 5),
            ("di`", 14),
            ("di]", 5),
            ("diB", 14),
            ("di>", 24),
            ("da»", 24),
            ("dab", 5),
        ] {
            let mut vim = normal();
            let outcome = feed(&mut vim, keys, &view(text, &parsed, at));
            match keys {
                // no guillemets and no parentheses on this line
                "da»" | "dab" => {
                    assert_eq!(outcome, Outcome::Swallow, "{keys}")
                }
                _ => {
                    assert!(
                        matches!(outcome, Outcome::Acts(_)),
                        "{keys}: {outcome:?}"
                    )
                }
            }
        }
    }

    #[test]
    fn a_verb_with_a_broken_noun_aborts() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        // d then an unbound key
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "dz", &sight), Outcome::Swallow);
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::Place(11)]),
            "the grammar recovered"
        );
        // d then a different verb
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "dy", &sight), Outcome::Swallow);
        // d then i then something that names nothing
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "diz", &sight), Outcome::Swallow);
        // a failed find under a verb
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "dfZ", &sight), Outcome::Swallow);
        // escape kills a pending verb, then still climbs
        let mut vim = normal();
        feed(&mut vim, "d", &sight);
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::empty(), &sight),
            Outcome::Swallow
        );
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::empty(), &sight),
            Outcome::Acts(vec![Act::Deactivate]),
        );
        // I A o O mean nothing behind a verb
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "dA", &sight), Outcome::Swallow);
    }

    #[test]
    fn charwise_yank_lands_on_the_spans_start() {
        let text = "un mot\n";
        assert_eq!(
            acts_of("yb", text, 3),
            vec![Act::SetClipboard("un ".into()), Act::Place(0)],
            "yank back moves to the start"
        );
        assert_eq!(
            acts_of("yw", text, 0),
            vec![Act::SetClipboard("un ".into()), Act::Place(0)],
        );
    }

    #[test]
    fn d_shorthand_on_an_empty_line_cuts_nothing() {
        assert_eq!(
            acts_of("D", NOTE, NOTE.len()),
            vec![Act::Splice {
                span: NOTE.len()..NOTE.len(),
                text: String::new(),
                caret: NOTE.len()
            }],
            "no clipboard clobber for an empty cut"
        );
    }

    #[test]
    fn semicolon_and_comma_carry_their_find_kind_under_a_verb() {
        let text = "un café, un café noir\n";
        let parsed = blocks::segment(text);
        // d; after fc repeats the inclusive find
        let mut vim = normal();
        feed(&mut vim, "fc", &view(text, &parsed, 0));
        let outcome = feed(&mut vim, "d;", &view(text, &parsed, 3));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("café, un c".into()),
                Act::Splice {
                    span: 3..14,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        // d, reverses into an exclusive backward find
        let mut vim = normal();
        feed(&mut vim, "fc", &view(text, &parsed, 0));
        let outcome = feed(&mut vim, "d,", &view(text, &parsed, 8));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 3..8,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        // dF is exclusive too
        let outcome = {
            let mut vim = normal();
            feed(&mut vim, "dFc", &view(text, &parsed, 8))
        };
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 3..8,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        // d; with nothing to repeat aborts whole
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "d;", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
        // dgg from below is linewise up
        let two = "une\ndeux\n";
        let parsed = blocks::segment(two);
        let outcome = {
            let mut vim = normal();
            feed(&mut vim, "dgg", &view(two, &parsed, 5))
        };
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("une\ndeux\n".into()),
                Act::Splice {
                    span: 0..9,
                    text: String::new(),
                    caret: 0
                },
            ]),
        );
    }

    #[test]
    fn r_and_tilde_edges_fail_quietly() {
        let parsed = blocks::segment(NOTE);
        // r with a character key carrying nothing
        let mut vim = normal();
        feed(&mut vim, "r", &view(NOTE, &parsed, 0));
        assert_eq!(
            vim.handle(
                &Key::Character(String::new()),
                Modifiers::empty(),
                &view(NOTE, &parsed, 0)
            ),
            Outcome::Swallow,
        );
        // ~ on the empty last line has nothing to flip
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "~", &view(NOTE, &parsed, NOTE.len())),
            Outcome::Swallow,
        );
        // ~ on an uppercase cluster lowers it
        let text = "É la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "~", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Splice {
                span: 0..2,
                text: "é".into(),
                caret: 2
            }]),
        );
    }

    // -- phase 4: visual mode ------------------------------------------------

    #[test]
    fn v_extends_by_motions_and_escape_returns_to_normal() {
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "v", &view(NOTE, &parsed, 2)),
            Outcome::Acts(vec![]),
        );
        assert_eq!(vim.mode, Mode::Visual(VisualKind::Char));
        // motions extend rather than collapse — arrows included
        assert_eq!(
            feed(&mut vim, "e", &spread(NOTE, &parsed, 2, 2)),
            Outcome::Acts(vec![Act::Extend(3)]),
        );
        assert_eq!(
            vim.handle(
                &Key::ArrowRight,
                Modifiers::empty(),
                &spread(NOTE, &parsed, 2, 3)
            ),
            Outcome::Acts(vec![Act::Extend(4)]),
        );
        // counts still compose
        assert_eq!(
            feed(&mut vim, "2j", &spread(NOTE, &parsed, 2, 4)),
            Outcome::Acts(vec![Act::Extend(27)]),
        );
        // o swaps the ends, unbound keys stay inert
        assert_eq!(
            feed(&mut vim, "o", &spread(NOTE, &parsed, 2, 27)),
            Outcome::Acts(vec![Act::SwapEnds]),
        );
        assert_eq!(
            feed(&mut vim, "z", &spread(NOTE, &parsed, 27, 2)),
            Outcome::Swallow,
        );
        // escape returns to normal with the caret at the head
        assert_eq!(
            vim.handle(
                &Key::Escape,
                Modifiers::empty(),
                &spread(NOTE, &parsed, 27, 2)
            ),
            Outcome::Acts(vec![Act::Place(2)]),
        );
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn visual_operators_take_the_selection() {
        let text = "un café noir\n";
        let parsed = blocks::segment(text);
        // v..d over "café": both end clusters included
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        let outcome = vim.handle(
            &character("d"),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 6),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 3..8,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        assert_eq!(vim.mode, Mode::Normal);
        // x is d; a backward selection spans the same
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        let outcome = vim.handle(
            &character("x"),
            Modifiers::empty(),
            &spread(text, &parsed, 6, 3),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 3..8,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        // c ends in insert
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        vim.handle(
            &character("c"),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 6),
        );
        assert_eq!(vim.mode, Mode::Insert);
        // y fills the register and lands at the span's start
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        let outcome = vim.handle(
            &character("y"),
            Modifiers::empty(),
            &spread(text, &parsed, 6, 3),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Place(3)
            ]),
        );
    }

    #[test]
    fn line_visual_takes_whole_lines_and_the_kinds_toggle() {
        let text = "une\ndeux\ntrois\n";
        let parsed = blocks::segment(text);
        // V then j then d: both lines and their newlines
        let mut vim = normal();
        feed(&mut vim, "V", &view(text, &parsed, 1));
        assert_eq!(vim.mode, Mode::Visual(VisualKind::Line));
        let outcome = vim.handle(
            &character("d"),
            Modifiers::empty(),
            &spread(text, &parsed, 1, 5),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("une\ndeux\n".into()),
                Act::Splice {
                    span: 0..9,
                    text: String::new(),
                    caret: 0
                },
            ]),
        );
        // v inside V switches kind; V again toggles out
        let mut vim = normal();
        feed(&mut vim, "V", &view(text, &parsed, 1));
        assert_eq!(
            feed(&mut vim, "v", &view(text, &parsed, 1)),
            Outcome::Acts(vec![]),
        );
        assert_eq!(vim.mode, Mode::Visual(VisualKind::Char));
        assert_eq!(
            feed(&mut vim, "V", &view(text, &parsed, 1)),
            Outcome::Acts(vec![]),
        );
        assert_eq!(vim.mode, Mode::Visual(VisualKind::Line));
        let outcome = feed(&mut vim, "V", &view(text, &parsed, 1));
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(1)]));
        assert_eq!(vim.mode, Mode::Normal);
        // and v toggles itself out too
        let mut vim = normal();
        feed(&mut vim, "v", &view(text, &parsed, 1));
        let outcome = feed(&mut vim, "v", &view(text, &parsed, 1));
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(1)]));
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn visual_finds_and_counts_still_answer() {
        let text = "un café, un café noir\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "v", &view(text, &parsed, 0));
        assert_eq!(
            feed(&mut vim, "fc", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Extend(3)]),
        );
        assert_eq!(
            feed(&mut vim, "2l", &spread(text, &parsed, 0, 3)),
            Outcome::Acts(vec![Act::Extend(5)]),
        );
        // a non-character key stays inert in visual
        assert_eq!(
            vim.handle(&Key::Tab, Modifiers::empty(), &view(text, &parsed, 5)),
            Outcome::Swallow,
        );
    }

    #[test]
    fn every_visual_motion_key_extends() {
        let text = "un café, un café noir\nposé là\n";
        let parsed = blocks::segment(text);
        // spot checks…
        let mut vim = normal();
        feed(&mut vim, "v", &view(text, &parsed, 10));
        assert_eq!(
            feed(&mut vim, "h", &spread(text, &parsed, 10, 10)),
            Outcome::Acts(vec![Act::Extend(9)]),
        );
        assert_eq!(
            feed(&mut vim, "k", &spread(text, &parsed, 10, 25)),
            Outcome::Acts(vec![Act::Extend(1)]),
        );
        assert_eq!(
            feed(&mut vim, "$", &spread(text, &parsed, 10, 0)),
            Outcome::Acts(vec![Act::Extend(22)]),
        );
        // …and the whole vocabulary answers without ever passing through
        for keys in ["w", "b", "0", "^", "G", ";", ",", "Fc", "Tc", "tc", "gg"]
        {
            let mut vim = normal();
            feed(&mut vim, "v", &view(text, &parsed, 10));
            let outcome = feed(&mut vim, keys, &spread(text, &parsed, 10, 10));
            assert_ne!(outcome, Outcome::Pass, "{keys}");
            assert!(matches!(vim.mode, Mode::Visual(_)), "{keys} left visual");
        }
        for key in [
            Key::ArrowLeft,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Home,
            Key::End,
        ] {
            let mut vim = normal();
            feed(&mut vim, "v", &view(text, &parsed, 10));
            let outcome = vim.handle(
                &key,
                Modifiers::empty(),
                &spread(text, &parsed, 10, 10),
            );
            assert!(
                matches!(outcome, Outcome::Acts(_)),
                "{key:?}: {outcome:?}"
            );
        }
    }

    #[test]
    fn p_and_capital_p_ask_for_the_clipboard() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "p", &sight),
            Outcome::Acts(vec![Act::Paste {
                before: false,
                count: 1
            }]),
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "3P", &sight),
            Outcome::Acts(vec![Act::Paste {
                before: true,
                count: 3
            }]),
        );
    }

    #[test]
    fn paste_spec_places_charwise_beside_the_caret() {
        let text = "un été\n";
        let parsed = blocks::segment(text);
        // p after the caret's cluster, landing on the last pasted cluster
        let (span, body, caret) =
            motions::paste_spec(text, &parsed, 0, "xy", false, 1);
        assert_eq!((span, body.as_str(), caret), (1..1, "xy", 2));
        // P at the caret
        let (span, body, caret) =
            motions::paste_spec(text, &parsed, 3, "xy", true, 1);
        assert_eq!((span, body.as_str(), caret), (3..3, "xy", 4));
        // a count repeats the body
        let (_, body, _) =
            motions::paste_spec(text, &parsed, 0, "ab", false, 2);
        assert_eq!(body, "abab");
    }

    #[test]
    fn paste_spec_opens_lines_for_linewise_clips() {
        let text = "une\ndeux\n";
        let parsed = blocks::segment(text);
        // p below the first line: at the second line's start
        let (span, body, caret) =
            motions::paste_spec(text, &parsed, 1, "  ligne\n", false, 1);
        assert_eq!((span, body.as_str(), caret), (4..4, "  ligne\n", 6));
        // P above it
        let (span, body, caret) =
            motions::paste_spec(text, &parsed, 1, "ligne\n", true, 1);
        assert_eq!((span, body.as_str(), caret), (0..0, "ligne\n", 0));
        // p on the note's last full line lands on the empty final line —
        // that newline is content, so nothing needs opening
        let (span, body, caret) =
            motions::paste_spec(text, &parsed, 5, "fin\n", false, 1);
        assert_eq!((span, body.as_str(), caret), (9..9, "fin\n", 9));
    }

    #[test]
    fn paste_below_a_blocks_last_line_opens_past_the_separator() {
        // the heading is its block's only line: its newline belongs to the
        // separator, so p opens with the break the body carried
        let parsed = blocks::segment(NOTE);
        let (span, body, caret) =
            motions::paste_spec(NOTE, &parsed, 2, "ligne\n", false, 1);
        assert_eq!((span, body.as_str(), caret), (9..9, "\nligne", 10));
    }
}
