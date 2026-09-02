//! The modal keymap between the widget and the editor — the slot
//! `editor.rs` names: the sink forwards keys here first,
//! and the grammar answers with editor intents, a swallow, or a pass back
//! to the phase-0 keymap. Pure over a snapshot of the note, so every rung,
//! entry, motion and operator tests headlessly
//! (adr/2026-08-escape-ladder-editor-wide-mode.md).

use std::ops::Range;

use dioxus::html::{Key, Modifiers};

use crate::blocks::Block;
use crate::caret;
use crate::motions::{self, FindKind, Lines, Motion, ObjectKind, Pattern};

/// Which grammar the keys speak. A note opens thinking — normal is the
/// birth mode, as vim's is, and i is one key away
/// (adr/2026-08-escape-ladder-editor-wide-mode.md).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    /// See the span before choosing the verb
    /// (adr/2026-08-visual-selection-is-the-anchor.md).
    Visual(VisualKind),
    /// R: each cluster overwrites the one under the caret until Escape
    /// (adr/2026-08-replace-mode-session-and-backspace.md).
    Replace,
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vim {
    pub mode: Mode,
    /// Digits swallowed before any operator; 0 means none.
    count: u32,
    /// Digits swallowed after the operator (d2w); the counts multiply.
    count2: u32,
    /// The verb waiting for its noun.
    operator: Option<Operator>,
    /// ys is arming: the noun about to resolve wraps a pair rather than
    /// yanking — operator stays Yank so every motion, count and object
    /// path resolves the noun unchanged
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    wrapping: bool,
    /// A key that awaits exactly one more key: g, f F t T, r, or the
    /// object side i/a after an operator.
    prefix: Option<Prefix>,
    /// The column a run of j and k holds through short lines, in clusters.
    goal: Option<usize>,
    /// What ; repeats and , reverses.
    last_find: Option<(FindKind, char)>,
    /// What . replays: the last change, recorded semantically
    /// (adr/2026-08-undo-at-vim-grain.md).
    last_change: Option<Change>,
    /// Where the open insert session began — what Escape captures as the
    /// session's typed text.
    insert_from: Option<usize>,
    /// Where the open R session began — what Escape captures as the
    /// session's typed text.
    replace_from: Option<usize>,
    /// The clusters the open R session has overwritten so far, in order —
    /// an empty entry means that keystroke appended past the line's end
    /// and overwrote nothing (adr/2026-08-replace-mode-session-and-backspace.md).
    replaced: Vec<String>,
    /// What n walks and N walks backward — the pattern / committed or *
    /// built (adr/2026-08-star-searches-whole-words.md).
    search: Option<Pattern>,
    /// What gv puts back: the kind and the two ends of the selection the
    /// last visual mode died with. Replayed as Place + Extend, because the
    /// selection *is* the anchor and needs no state of its own
    /// (adr/2026-08-visual-gains-p-r-s-and-gv.md).
    last_visual: Option<(VisualKind, usize, usize)>,
}

/// One recorded change, semantic rather than keystrokes: . resolves it
/// again at the caret it finds.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Change {
    Operate {
        op: Operator,
        noun: Noun,
        count: usize,
        /// What a c-change's insert session typed, captured at Escape.
        typed: String,
    },
    Cut {
        forward: bool,
        count: usize,
    },
    Replace {
        ch: char,
        count: usize,
    },
    Toggle {
        count: usize,
    },
    /// J and gJ (adr/2026-08-join-walks-the-visible-rows.md).
    Join {
        spaced: bool,
        count: usize,
    },
    Paste {
        before: bool,
        count: usize,
    },
    Insert {
        entry: InsertEntry,
        typed: String,
    },
    /// R: the session's overwritten text, captured at Escape — the dot
    /// replays it as one splice
    /// (adr/2026-08-replace-mode-session-and-backspace.md).
    Overwrite {
        typed: String,
    },
    /// cs / ds: the old pair replaced or removed — new is the pair key
    /// typed, or None for ds
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    Resurround {
        old: ObjectKind,
        new: Option<String>,
    },
    /// ys<noun><pair>: the noun that named the span to wrap, and the pair
    /// key typed — None while the pair key is still pending
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    Wrap {
        noun: WrapNoun,
        count: usize,
        pair: Option<String>,
    },
}

/// An operator's recorded noun.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Noun {
    Motion(Motion),
    Object { kind: ObjectKind, around: bool },
    Lines,
    Clusters,
}

/// A wrap's recorded noun: an operator's, minus the clusters `s` alone
/// owns — ys spells its noun through a motion, an object or the doubled
/// `yss`, never through `Noun::Clusters`, so the record cannot spell it
/// either and the dot has no impossible case to refuse
/// (adr/2026-08-clusters-noun-belongs-to-s.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WrapNoun {
    Motion(Motion),
    Object { kind: ObjectKind, around: bool },
    Lines,
}

impl From<WrapNoun> for Noun {
    fn from(noun: WrapNoun) -> Self {
        match noun {
            WrapNoun::Motion(motion) => Noun::Motion(motion),
            WrapNoun::Object { kind, around } => Noun::Object { kind, around },
            WrapNoun::Lines => Noun::Lines,
        }
    }
}

/// The six insert entries, recorded for replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InsertEntry {
    Before,
    After,
    FirstNonBlank,
    LineEnd,
    Below,
    Above,
}

/// The verbs: three that move text through the one register
/// (adr/2026-08-one-register-the-clipboard.md), and three that rewrite it
/// in place without touching the clipboard at all
/// (adr/2026-08-case-operators-are-verbs.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
    /// gu gU g~ — armed behind g, doubled as guu gUU g~~.
    Lower,
    Upper,
    Flip,
}

impl Operator {
    /// Whether the verb only recases what it spans: no register write, no
    /// insert session, the caret left at the span's start.
    fn recasing(self) -> bool {
        matches!(self, Operator::Lower | Operator::Upper | Operator::Flip)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Prefix {
    Go,
    /// z: zz zt zb name where the caret should sit in the pane
    /// (adr/2026-08-scroll-anchor-is-consumed-once.md).
    Scroll,
    Find(FindKind),
    Replace,
    Object {
        around: bool,
    },
    /// cs / ds: the target pair key is next; replace tells cs from ds
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    Surround {
        replace: bool,
    },
    /// cs<old>: the new pair key is next.
    SurroundNew {
        old: ObjectKind,
    },
    /// ys<noun> or visual S: the noun's span is resolved, the pair key is
    /// next. `record` marks the ys path, whose pending `Change::Wrap`
    /// waits for that key to fill its pair slot — visual S recorded no
    /// change and must leave whichever one stands alone
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    Wrap {
        span: Range<usize>,
        record: bool,
    },
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
    Paste {
        before: bool,
        count: usize,
    },
    /// Visual's motion: the head moves, the anchor holds.
    Extend(usize),
    /// o in visual: the caret jumps to the selection's other end.
    SwapEnds,
    /// One change intent begins: the editor snapshots itself
    /// (adr/2026-08-undo-at-vim-grain.md).
    Checkpoint,
    /// u and Ctrl+R.
    Undo,
    Redo,
    /// / — the widget opens its one-line prompt
    /// (adr/2026-08-search-lands-through-place.md).
    OpenSearch,
    /// : — the same prompt, wearing the other sigil; `prefill` carries the
    /// visual range so a selection survives the mode change
    /// (adr/2026-08-ex-line-is-literal-and-global.md).
    OpenEx {
        prefill: String,
    },
    /// :w — the buffer reaches the disk now rather than at the next pause.
    Save,
    /// gf — the link under the caret opens, through the one follow path
    /// Ctrl+Enter and the palette already share
    /// (adr/2026-08-gf-follows-the-link.md).
    FollowLink,
    /// zz zt zb, and the centring every j and k asks for: where the caret
    /// should sit in the pane for the next mount, and only that one
    /// (adr/2026-08-scroll-anchor-is-consumed-once.md).
    Scroll(Anchor),
    /// v after an operator's span is spent: the selection comes back
    /// (adr/2026-08-visual-gains-p-r-s-and-gv.md).
    PasteOver {
        span: Range<usize>,
        linewise: bool,
    },
    /// Plain j and k walk the lines the webview draws, which the grammar
    /// cannot see: the executor resolves the landing.
    WalkVisual {
        down: bool,
        count: usize,
        extend: bool,
    },
}

/// Where a scroll should put the caret in the pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Anchor {
    /// The stock behaviour: a caret already in view scrolls nothing.
    #[default]
    Nearest,
    Center,
    Top,
    Bottom,
}

/// What a submitted `:` line resolved to. A command line that fails must
/// never fall silent the way an unbound keystroke does, so the failure is a
/// sentence for the status surface rather than a swallow (AIR ERR-2,
/// adr/2026-08-ex-line-is-literal-and-global.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExOutcome {
    Acts(Vec<Act>),
    Refused(String),
}

/// The ranges the ex line understands, and no arithmetic beyond them
/// (adr/2026-08-ex-line-is-literal-and-global.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExRange {
    /// No range at all: the caret's own line.
    Line,
    /// `%`
    Whole,
    /// `'<,'>` — the rows the selection covered, which a visual `:` spells
    /// into the prompt for you.
    Selection,
    /// `N` or `N,M`, one-based as vim counts.
    Rows(usize, usize),
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
        // a modifier's own keydown is key state, not a keystroke: the
        // webview sends one before every shifted or AltGr character, and
        // normal mode's catch-all would read it as an unbound key and
        // reset the pending grammar — killing d$, di", dF, cs" and every
        // other chord whose continuation needs a modifier. Pass rather
        // than Swallow: a bare modifier must keep its default, or native
        // shift-selection and the dead-key machinery lose their ground
        // (adr/2026-08-modifier-keydowns-are-key-state.md).
        if matches!(
            key,
            Key::Shift
                | Key::Control
                | Key::Alt
                | Key::AltGraph
                | Key::Meta
                | Key::CapsLock
        ) {
            return Outcome::Pass;
        }
        if modifiers.ctrl() || modifiers.meta() {
            // the one ctrl carve-out: redo, which no palette chord uses
            // (adr/2026-08-undo-at-vim-grain.md)
            if self.mode == Mode::Normal
                && modifiers.ctrl()
                && !modifiers.meta()
                && *key == Key::Character("r".to_string())
            {
                return Outcome::Acts(vec![Act::Redo]);
            }
            return Outcome::Pass;
        }
        // one seam remembers the selection for gv: every keystroke that
        // reaches visual mode sees the live anchor and head, so whatever
        // the *last* such keystroke saw is exactly the selection the mode
        // died with — no bookkeeping at each of the half-dozen exits
        // (adr/2026-08-visual-gains-p-r-s-and-gv.md)
        if let Mode::Visual(kind) = self.mode {
            self.last_visual = Some((kind, view.anchor, view.head));
        }
        // shift+Escape is the one way out of a note: the mode closes its
        // own session exactly as a plain Escape would — insert's caret
        // step-back, R's clipboard write — and the block renders again
        // behind it (adr/2026-08-shift-escape-leaves-the-note.md)
        if *key == Key::Escape && modifiers.shift() {
            let mut acts =
                if let Outcome::Acts(acts) = self.mode_key(key, view) {
                    acts
                } else {
                    Vec::new()
                };
            acts.push(Act::Deactivate);
            return Outcome::Acts(acts);
        }
        // Tab is `>>` under another name, in every mode but R, whose
        // session overwrites cluster by cluster and could not survive its
        // line moving under it (adr/2026-08-tab-indents-in-every-mode.md)
        if *key == Key::Tab && self.mode != Mode::Replace {
            if self.operator.is_some() || self.prefix.is_some() {
                // a stray Tab behind an armed verb aborts it, as every
                // other key that cannot be its noun does
                self.reset();
                return Outcome::Swallow;
            }
            return self.indent(!modifiers.shift(), view);
        }
        self.mode_key(key, view)
    }

    /// Tab and Shift+Tab: the caret's line — every selected line in visual
    /// — moves one level in or out, whatever column the caret sits at. One
    /// splice, so one checkpoint reverses the whole press; a press with
    /// nothing to move (a bare line dedenting, a blank line either way)
    /// swallows rather than checkpointing an empty change
    /// (adr/2026-08-tab-indents-in-every-mode.md).
    fn indent(&mut self, deeper: bool, view: &View) -> Outcome {
        let lines = Lines::of(view.text, view.blocks);
        let (from, to) = if let Mode::Visual(_) = self.mode {
            // vim's own > spends the selection and leaves visual behind
            self.mode = Mode::Normal;
            (view.anchor.min(view.head), view.anchor.max(view.head))
        } else {
            (view.head, view.head)
        };
        let (first, last) = (lines.row_of(from), lines.row_of(to));
        self.reset();
        let span = lines.row(first).start..lines.row(last).end;
        let mut text = String::new();
        let mut at = span.start;
        let mut drift = 0isize;
        let mut floor = view.head;
        let mut moved = false;
        for index in first..=last {
            let row = lines.row(index);
            // the bytes between two rows — the newline, or a block
            // separator no line owns — ride along untouched
            text.push_str(view.text.get(at..row.start).unwrap_or_default());
            let body = view.text.get(row.clone()).unwrap_or_default();
            let spaces = body.len() - body.trim_start_matches(' ').len();
            let width = indented(body, spaces, deeper);
            text.push_str(&" ".repeat(width));
            text.push_str(body.get(spaces..).unwrap_or_default());
            moved |= width != spaces;
            if row.start <= view.head {
                floor = row.start.saturating_add_signed(drift);
                drift += width as isize - spaces as isize;
            }
            at = row.end;
        }
        if !moved {
            return Outcome::Swallow;
        }
        // insert mode splices at the line start, which can sit before the
        // session's own: the dot replays `text[insert_from..head]`, so the
        // mark rides the same shift its text did. Only insert mode holds
        // one, and it moves exactly one row.
        if let Some(from) = self.insert_from.filter(|from| span.start <= *from)
        {
            self.insert_from = Some(from.saturating_add_signed(drift));
        }
        let caret = view.head.saturating_add_signed(drift).max(floor);
        Outcome::Acts(vec![Act::Checkpoint, Act::Splice { span, text, caret }])
    }

    /// The keystroke as the current mode reads it — the dispatch shift+
    /// Escape borrows to close a session before it leaves the note.
    fn mode_key(&mut self, key: &Key, view: &View) -> Outcome {
        match self.mode {
            Mode::Insert => self.insert_key(key, view),
            Mode::Normal => self.normal_key(key, view),
            Mode::Visual(kind) => self.visual_key(kind, key, view),
            Mode::Replace => self.replace_key(key, view),
        }
    }

    /// Insert mode is phase 0's writing flow, untouched but for two keys
    /// the grammar owns: Tab, handled a level up, and Escape — back to
    /// normal, the caret stepping onto the last cluster of what was just
    /// typed, as vim leaves it.
    fn insert_key(&mut self, key: &Key, view: &View) -> Outcome {
        if *key != Key::Escape {
            return Outcome::Pass;
        }
        self.mode = Mode::Normal;
        // the session's typed text, for the dot to replay — empty when
        // the caret wandered backward past its start
        if let Some(from) = self.insert_from.take() {
            let typed = view
                .text
                .get(from..view.head)
                .unwrap_or_default()
                .to_string();
            match &mut self.last_change {
                Some(Change::Insert { typed: slot, .. }) => *slot = typed,
                Some(Change::Operate {
                    op: Operator::Change,
                    typed: slot,
                    ..
                }) => *slot = typed,
                _ => {}
            }
        }
        let line = Lines::of(view.text, view.blocks).around(view.head);
        let target = if view.head > line.start {
            caret::prev_cluster(view.text, view.head).max(line.start)
        } else {
            view.head
        };
        Outcome::Acts(vec![Act::Place(target)])
    }

    /// R mode: a typed cluster overwrites the one under the caret,
    /// appending once the line's end is reached; Backspace restores what
    /// the session overwrote, then only moves once nothing is left to
    /// restore; Escape closes the session onto the clipboard and steps
    /// back one cluster, as insert's does
    /// (adr/2026-08-replace-mode-session-and-backspace.md).
    fn replace_key(&mut self, key: &Key, view: &View) -> Outcome {
        match key {
            Key::Character(cluster) => {
                let line = Lines::of(view.text, view.blocks).around(view.head);
                // past the line's end — where an opened note's caret sits,
                // after its trailing newline — there is nothing to
                // overwrite and the span would run backwards: R inserts
                // there, as vim's does (found by the key-sequence property)
                let end = caret::next_cluster(view.text, view.head)
                    .min(line.end)
                    .max(view.head);
                self.replaced.push(
                    view.text
                        .get(view.head..end)
                        .unwrap_or_default()
                        .to_string(),
                );
                Outcome::Acts(vec![Act::Splice {
                    span: view.head..end,
                    text: cluster.clone(),
                    caret: view.head + cluster.len(),
                }])
            }
            // the stack alone answers whether the cluster behind the caret
            // is the session's to restore: every keystroke that overwrites
            // pushes at the caret and steps right, and every Backspace
            // either pops the cluster it just left behind or moves without
            // touching the stack — so a non-empty stack always has its top
            // sitting immediately behind the caret, session start or not
            // (adr/2026-08-replace-mode-session-and-backspace.md)
            Key::Backspace => match self.replaced.pop() {
                Some(original) => {
                    let span =
                        caret::prev_cluster(view.text, view.head)..view.head;
                    let caret = span.start;
                    Outcome::Acts(vec![Act::Splice {
                        span,
                        text: original,
                        caret,
                    }])
                }
                None => {
                    let line =
                        Lines::of(view.text, view.blocks).around(view.head);
                    Outcome::Acts(vec![Act::Place(
                        caret::prev_cluster(view.text, view.head)
                            .max(line.start),
                    )])
                }
            },
            Key::Escape => {
                self.mode = Mode::Normal;
                // the session's typed text, for the dot to replay — empty
                // when the caret wandered backward past its start, exactly
                // as insert's Escape captures its own session. begin_replace
                // sets replace_from together with the Overwrite record, so
                // the two never drift apart within one session.
                let from = self.replace_from.take().unwrap_or(view.head);
                let typed = view
                    .text
                    .get(from..view.head)
                    .unwrap_or_default()
                    .to_string();
                self.last_change = Some(Change::Overwrite { typed });
                let mut acts = Vec::new();
                // the session's overwritten text reaches the one register
                // once, here — never per keystroke
                // (adr/2026-08-one-register-the-clipboard.md)
                let overwritten: String = self.replaced.drain(..).collect();
                if !overwritten.is_empty() {
                    acts.push(Act::SetClipboard(overwritten));
                }
                let line = Lines::of(view.text, view.blocks).around(view.head);
                let target = if view.head > line.start {
                    caret::prev_cluster(view.text, view.head).max(line.start)
                } else {
                    view.head
                };
                acts.push(Act::Place(target));
                Outcome::Acts(acts)
            }
            // the arrows must not walk the caret out from under the
            // session's bookkeeping
            _ => Outcome::Swallow,
        }
    }

    /// Normal mode: counts, verbs and prefixes accumulate; motions,
    /// objects and entries resolve; Escape kills the pending grammar and
    /// then goes inert — and everything unbound is inert too. AltGr
    /// characters carry alt, which is why the printable swallow must see
    /// them too.
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
            // and there the ladder stops: a plain Escape never leaves the
            // note, so the reflex of pressing it to be sure of the mode
            // costs nothing (adr/2026-08-shift-escape-leaves-the-note.md)
            Key::Escape => Outcome::Swallow,
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
            // everything left is a platform key, not a keystroke the user
            // aimed at the grammar: a dead key, an F-key, or whatever
            // Dioxus could not parse and handed over as `Unidentified`
            // (dioxus-html keyboard.rs: `.unwrap_or(Key::Unidentified)`).
            // Swallowed like visual mode's, and pointedly without a reset
            // — a chord must not lose its verb to the layout machinery
            // (adr/2026-08-platform-keys-do-not-abort-a-chord.md). An
            // unbound *character* still aborts, below, as vim's does.
            _ => Outcome::Swallow,
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
            // ysy is nothing: vim-surround doubles the s, not the y, so the
            // stray y aborts exactly as a mismatched pair of verbs does
            // (adr/2026-08-surround-pair-set-and-padding.md)
            "y" if self.wrapping => {
                self.reset();
                Outcome::Swallow
            }
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
            // plain j/k walk the webview's wrapped lines, which the
            // grammar cannot see; behind an operator they stay vim's
            // linewise dj/yk/cj over logical lines
            // (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
            "j" => match self.operator {
                Some(_) => self.run_motion(Motion::Down, view),
                None => self.walk_visual(true),
            },
            "k" => match self.operator {
                Some(_) => self.run_motion(Motion::Up, view),
                None => self.walk_visual(false),
            },
            "w" => self.run_motion(Motion::WordForward, view),
            "b" => self.run_motion(Motion::WordBack, view),
            "e" => self.run_motion(Motion::WordEnd, view),
            "W" => self.run_motion(Motion::BigWordForward, view),
            "B" => self.run_motion(Motion::BigWordBack, view),
            "E" => self.run_motion(Motion::BigWordEnd, view),
            "%" => self.run_motion(Motion::MatchPair, view),
            "0" => self.run_motion(Motion::LineStart, view),
            "^" => self.run_motion(Motion::FirstNonBlank, view),
            "$" => self.run_motion(Motion::LineEnd, view),
            "G" => self.run_motion(Motion::LastLine, view),
            ";" => self.run_motion(Motion::RepeatFind, view),
            "," => self.run_motion(Motion::RepeatFindBack, view),
            // the doubled case verbs: guu gUU g~~, whose second key is not
            // a verb key and so cannot reach operator_key's doubling rule.
            // Ahead of u's undo and ~'s single-cluster flip, both of which
            // only answer with no verb pending
            // (adr/2026-08-case-operators-are-verbs.md)
            "u" if self.operator == Some(Operator::Lower) => {
                self.current_lines(Operator::Lower, view)
            }
            "U" if self.operator == Some(Operator::Upper) => {
                self.current_lines(Operator::Upper, view)
            }
            "~" if self.operator == Some(Operator::Flip) => {
                self.current_lines(Operator::Flip, view)
            }
            "J" if self.operator.is_none() => self.join(true, view),
            "*" if self.operator.is_none() => self.search_word(true, view),
            "#" if self.operator.is_none() => self.search_word(false, view),
            "z" if self.operator.is_none() => {
                self.await_prefix(Prefix::Scroll)
            }
            ":" if self.operator.is_none() => {
                self.reset();
                Outcome::Acts(vec![Act::OpenEx {
                    prefill: String::new(),
                }])
            }
            "i" => self.begin_insert(InsertEntry::Before, view, &line),
            "a" => self.begin_insert(InsertEntry::After, view, &line),
            "I" if self.operator.is_none() => {
                self.begin_insert(InsertEntry::FirstNonBlank, view, &line)
            }
            "A" if self.operator.is_none() => {
                self.begin_insert(InsertEntry::LineEnd, view, &line)
            }
            "o" if self.operator.is_none() => {
                self.begin_insert(InsertEntry::Below, view, &line)
            }
            "O" if self.operator.is_none() => {
                self.begin_insert(InsertEntry::Above, view, &line)
            }
            "u" if self.operator.is_none() => {
                self.reset();
                Outcome::Acts(vec![Act::Undo])
            }
            "." if self.operator.is_none() => self.repeat(view),
            "/" if self.operator.is_none() => {
                self.reset();
                Outcome::Acts(vec![Act::OpenSearch])
            }
            "n" if self.operator.is_none() => self.search_jump(true, view),
            "N" if self.operator.is_none() => self.search_jump(false, view),
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
            // yss: vim-surround's own doubled key — the current line is the
            // noun, recorded and checkpointed like every other wrap
            // (adr/2026-08-surround-pair-set-and-padding.md)
            "s" if self.wrapping => self.current_lines(Operator::Yank, view),
            // ys: the operator stays Yank so every existing motion, count
            // and object path below resolves the noun unchanged — only
            // finish_operator, at the end, knows the noun is wrapping
            // rather than yanking (adr/2026-08-surround-pair-set-and-padding.md).
            "s" if self.operator == Some(Operator::Yank) => {
                self.wrapping = true;
                Outcome::Swallow
            }
            "s" if self.operator == Some(Operator::Delete) => {
                self.await_prefix(Prefix::Surround { replace: false })
            }
            "s" if self.operator == Some(Operator::Change) => {
                self.await_prefix(Prefix::Surround { replace: true })
            }
            "s" if self.operator.is_none() => {
                self.change_clusters(view, &line)
            }
            "S" if self.operator.is_none() => {
                self.current_lines(Operator::Change, view)
            }
            "R" if self.operator.is_none() => self.begin_replace(view),
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
            // s is c, as vim's is. S is not vim's line-wise change here,
            // because surround owns it — which is exactly what vim-surround
            // does to the same key in a real vim
            // (adr/2026-08-visual-gains-p-r-s-and-gv.md)
            "c" | "s" => self.visual_operate(Operator::Change, kind, view),
            "y" => self.visual_operate(Operator::Yank, kind, view),
            "u" => self.visual_operate(Operator::Lower, kind, view),
            "U" => self.visual_operate(Operator::Upper, kind, view),
            "~" => self.visual_operate(Operator::Flip, kind, view),
            "r" => self.await_prefix(Prefix::Replace),
            "p" => self.visual_paste(kind, view),
            // the objects reach visual: i and a name a span the selection
            // takes rather than one a verb eats
            // (adr/2026-08-objects-reach-visual-mode.md)
            "i" => self.await_prefix(Prefix::Object { around: false }),
            "a" => self.await_prefix(Prefix::Object { around: true }),
            "z" => self.await_prefix(Prefix::Scroll),
            // the range comes spelled, as vim spells it
            // (adr/2026-08-ex-line-is-literal-and-global.md)
            ":" => Outcome::Acts(vec![Act::OpenEx {
                prefill: "'<,'>".to_string(),
            }]),
            // S: the same span the verbs take, wrapped instead of cut — no
            // Change recorded, the dot after S replays whatever change came
            // before, deliberately. The mode stays visual until the pair
            // key lands, so the selection keeps its owner — and its
            // line-wise highlight — for that keystroke
            // (adr/2026-08-surround-pair-set-and-padding.md).
            "S" => {
                let (span, _) = visual_span(kind, view);
                self.await_prefix(Prefix::Wrap {
                    span,
                    record: false,
                })
            }
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
            // no verb can be pending here — visual's own verbs resolve on
            // the keystroke that names them — so j and k always walk the
            // webview's wrapped lines, as they do in normal mode
            "j" => self.walk_visual(true),
            "k" => self.walk_visual(false),
            "w" => self.run_motion(Motion::WordForward, view),
            "b" => self.run_motion(Motion::WordBack, view),
            "e" => self.run_motion(Motion::WordEnd, view),
            "W" => self.run_motion(Motion::BigWordForward, view),
            "B" => self.run_motion(Motion::BigWordBack, view),
            "E" => self.run_motion(Motion::BigWordEnd, view),
            "%" => self.run_motion(Motion::MatchPair, view),
            "0" => self.run_motion(Motion::LineStart, view),
            "^" => self.run_motion(Motion::FirstNonBlank, view),
            "$" => self.run_motion(Motion::LineEnd, view),
            "G" => self.run_motion(Motion::LastLine, view),
            ";" => self.run_motion(Motion::RepeatFind, view),
            "," => self.run_motion(Motion::RepeatFindBack, view),
            _ => Outcome::Swallow,
        }
    }

    /// A verb over the selection. The mode falls back to normal — or into
    /// insert, when the verb was c.
    fn visual_operate(
        &mut self,
        op: Operator,
        kind: VisualKind,
        view: &View,
    ) -> Outcome {
        let (span, linewise) = visual_span(kind, view);
        self.mode = Mode::Normal;
        self.finish_operator(op, span, linewise, view)
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
            // a platform key is not the answer the prefix is waiting for,
            // and must not be mistaken for a wrong one: keep waiting
            // (adr/2026-08-platform-keys-do-not-abort-a-chord.md)
            if !matches!(key, Key::Escape) {
                self.prefix = Some(prefix);
                return Outcome::Swallow;
            }
            // a pending wrap still owns the visual selection: Escape aborts
            // it to normal exactly as an unknown pair character does, so one
            // Escape is enough either way
            // (adr/2026-08-surround-pair-set-and-padding.md)
            if matches!(prefix, Prefix::Wrap { .. }) {
                self.mode = Mode::Normal;
            }
            self.reset();
            return Outcome::Swallow;
        };
        match prefix {
            Prefix::Go if character == "g" => {
                self.run_motion(Motion::FirstLine, view)
            }
            Prefix::Go if character == "e" => {
                self.run_motion(Motion::WordEndBack, view)
            }
            Prefix::Go if character == "E" => {
                self.run_motion(Motion::BigWordEndBack, view)
            }
            // the doubled forms guu gUU g~~ cannot reach operator_key's
            // own doubling rule, because their second key is not a verb key
            // (adr/2026-08-case-operators-are-verbs.md)
            Prefix::Go if character == "u" => {
                self.case_key(Operator::Lower, view)
            }
            Prefix::Go if character == "U" => {
                self.case_key(Operator::Upper, view)
            }
            Prefix::Go if character == "~" => {
                self.case_key(Operator::Flip, view)
            }
            // gJ joins without the space, and gf opens what the caret
            // stands on (adr/2026-08-gf-follows-the-link.md)
            Prefix::Go if character == "J" && self.operator.is_none() => {
                self.join(false, view)
            }
            Prefix::Go if character == "f" && self.operator.is_none() => {
                self.reset();
                Outcome::Acts(vec![Act::FollowLink])
            }
            Prefix::Go if character == "v" && self.operator.is_none() => {
                self.reselect()
            }
            // z: zz zt zb, and nothing else — this editor has no folds, so
            // za and zR stay inert rather than pretending
            // (adr/2026-08-scroll-anchor-is-consumed-once.md)
            Prefix::Scroll => {
                self.reset();
                match character.as_str() {
                    "z" => Outcome::Acts(vec![Act::Scroll(Anchor::Center)]),
                    "t" => Outcome::Acts(vec![Act::Scroll(Anchor::Top)]),
                    "b" => Outcome::Acts(vec![Act::Scroll(Anchor::Bottom)]),
                    _ => Outcome::Swallow,
                }
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
            Prefix::Surround { replace } => {
                let Some(kind) = surround_kind(character) else {
                    self.reset();
                    return Outcome::Swallow;
                };
                if replace {
                    self.await_prefix(Prefix::SurroundNew { old: kind })
                } else {
                    self.apply_surround(kind, None, view)
                }
            }
            Prefix::SurroundNew { old } => match surround_pair(character) {
                Some(_) => {
                    self.apply_surround(old, Some(character.to_string()), view)
                }
                None => {
                    self.reset();
                    Outcome::Swallow
                }
            },
            Prefix::Wrap { span, record } => {
                self.wrap_span(span, character, record, view)
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

    /// dd cc yy, Y and yss: the operator over [count] whole lines — the
    /// wrap records its own change class, since ys left the operator Yank
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    fn current_lines(&mut self, op: Operator, view: &View) -> Outcome {
        let lines = Lines::of(view.text, view.blocks);
        let total = self.effective_count();
        let first = lines.row_of(view.head);
        let last = (first + total - 1).min(lines.rows() - 1);
        let span = if self.wrapping {
            self.record(Change::Wrap {
                noun: WrapNoun::Lines,
                count: total,
                pair: None,
            });
            // the wrap stops at the last line's text: the ending belongs to
            // the line, and a newline inside the pair would land the closing
            // delimiter at the head of the next line
            // (adr/2026-08-surround-pair-set-and-padding.md)
            lines.row(first).start..lines.row(last).end
        } else {
            if op != Operator::Yank {
                self.record(Change::Operate {
                    op,
                    noun: Noun::Lines,
                    count: total,
                    typed: String::new(),
                });
            }
            motions::linewise_span(&lines, first, last)
        };
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

    /// Plain j/k: the grammar cannot see how the webview wraps the note's
    /// lines, so it hands the direction and count to the executor rather
    /// than resolving a landing itself — the goal column moves with it,
    /// becoming a pixel x a later task holds across the run instead of
    /// this struct's logical-cluster `goal`
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    fn walk_visual(&mut self, down: bool) -> Outcome {
        let count = self.count.max(1) as usize;
        let extend = matches!(self.mode, Mode::Visual(_));
        self.reset();
        Outcome::Acts(vec![Act::WalkVisual {
            down,
            count,
            extend,
        }])
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
        // cw acts as ce on a word, and cW as cE — vim's own quirk, kept
        // for both sizes
        // (adr/2026-08-word-motions-add-their-big-siblings.md)
        let motion = match motion {
            Motion::WordForward if op == Operator::Change && on_word => {
                Motion::WordEnd
            }
            Motion::BigWordForward if op == Operator::Change && on_word => {
                Motion::BigWordEnd
            }
            other => other,
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
        if self.wrapping {
            self.record(Change::Wrap {
                noun: WrapNoun::Motion(motion),
                count: total,
                pair: None,
            });
        } else if op != Operator::Yank {
            self.record(Change::Operate {
                op,
                noun: Noun::Motion(motion),
                count: total,
                typed: String::new(),
            });
        }
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
                // dw stops at the line's end rather than eating the
                // break; dW likewise
                if matches!(
                    motion,
                    Motion::WordForward | Motion::BigWordForward
                ) {
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
        let Some(kind) = object_kind(character) else {
            self.reset();
            return Outcome::Swallow;
        };
        // in visual the object names a span the *selection* takes, so the
        // anchor lands on one end and the head on the other — the same
        // Place-then-Extend pair gv uses, and no verb is involved
        // (adr/2026-08-objects-reach-visual-mode.md)
        if let Mode::Visual(visual) = self.mode {
            self.reset();
            let Some(span) = motions::object(
                view.text,
                view.blocks,
                view.head,
                kind,
                around,
            ) else {
                return Outcome::Swallow;
            };
            return Outcome::Acts(select_span(span, visual, view));
        }
        let Some(op) = self.operator else {
            self.reset();
            return Outcome::Swallow;
        };
        self.object_noun(op, kind, around, view)
    }

    /// gu gU g~: arm the verb, or take the current lines when the same one
    /// is already armed — gugu and gUgU alongside guu and gUU
    /// (adr/2026-08-case-operators-are-verbs.md).
    fn case_key(&mut self, op: Operator, view: &View) -> Outcome {
        match self.operator {
            Some(pending) if pending == op => self.current_lines(op, view),
            Some(_) => {
                self.reset();
                Outcome::Swallow
            }
            None => {
                self.operator = Some(op);
                Outcome::Swallow
            }
        }
    }

    /// gv: the selection the last visual mode died with comes back. The
    /// selection *is* phase 0's anchor, so restoring it is a Place on one
    /// end and an Extend to the other — it needs no act of its own
    /// (adr/2026-08-visual-gains-p-r-s-and-gv.md).
    fn reselect(&mut self) -> Outcome {
        self.reset();
        let Some((kind, anchor, head)) = self.last_visual else {
            return Outcome::Swallow;
        };
        self.mode = Mode::Visual(kind);
        Outcome::Acts(vec![Act::Place(anchor), Act::Extend(head)])
    }

    /// Visual p: the clipboard replaces the span, and the *replaced* text
    /// does not go back out. Vim clobbers its unnamed register here, but
    /// the register is the OS clipboard
    /// (adr/2026-08-one-register-the-clipboard.md), so clobbering would
    /// spend the clip on its first use and break pasting one thing over
    /// several spans — which is what visual p is for
    /// (adr/2026-08-visual-gains-p-r-s-and-gv.md).
    fn visual_paste(&mut self, kind: VisualKind, view: &View) -> Outcome {
        let (span, linewise) = visual_span(kind, view);
        self.reset();
        self.mode = Mode::Normal;
        Outcome::Acts(vec![Act::Checkpoint, Act::PasteOver { span, linewise }])
    }

    /// The object side of `finish_object`, taking the kind directly —
    /// what `repeat_operate` and `resolve_noun` re-run too, since a
    /// replayed object noun is never a raw character to look up
    /// (adr/2026-08-undo-at-vim-grain.md).
    fn object_noun(
        &mut self,
        op: Operator,
        kind: ObjectKind,
        around: bool,
        view: &View,
    ) -> Outcome {
        match motions::object(view.text, view.blocks, view.head, kind, around)
        {
            Some(span) => {
                let linewise = matches!(kind, ObjectKind::Block);
                if self.wrapping {
                    self.record(Change::Wrap {
                        noun: WrapNoun::Object { kind, around },
                        count: 1,
                        pair: None,
                    });
                } else if op != Operator::Yank {
                    self.record(Change::Operate {
                        op,
                        noun: Noun::Object { kind, around },
                        count: 1,
                        typed: String::new(),
                    });
                }
                self.finish_operator(op, span, linewise, view)
            }
            None => {
                self.reset();
                Outcome::Swallow
            }
        }
    }

    /// cs / ds: the innermost `old` pair loses both delimiters — ds drops
    /// them onto the register, cs replaces them bare with the pair `new`
    /// names; a missing target aborts quietly, exactly as a broken text
    /// object does (adr/2026-08-surround-pair-set-and-padding.md).
    fn apply_surround(
        &mut self,
        old: ObjectKind,
        new: Option<String>,
        view: &View,
    ) -> Outcome {
        self.reset();
        let Some((open, close)) =
            motions::surround_spans(view.text, view.blocks, view.head, old)
        else {
            return Outcome::Swallow;
        };
        let inner = view.text.get(open.end..close.start).unwrap_or_default();
        let cut = format!(
            "{}{}",
            view.text.get(open.start..open.end).unwrap_or_default(),
            view.text.get(close.start..close.end).unwrap_or_default(),
        );
        // cs writes its new delimiters bare, never re-padded — only a
        // fresh wrap (ys/S) ever adds the space
        // (adr/2026-08-surround-pair-set-and-padding.md)
        let replacement = match new.as_deref().and_then(surround_pair) {
            Some((new_open, new_close, _)) => {
                format!("{new_open}{inner}{new_close}")
            }
            None => inner.to_string(),
        };
        let span = open.start..close.end;
        let caret = open.start;
        self.record(Change::Resurround { old, new });
        Outcome::Acts(vec![
            Act::Checkpoint,
            Act::SetClipboard(cut),
            Act::Splice {
                span,
                text: replacement,
                caret,
            },
        ])
    }

    /// The pair key ys and visual S were waiting for: the noun's span
    /// gets wrapped with the pair's delimiters, padded only when the key
    /// opens a bracket — the yank register never fills, since wrapping
    /// isn't a cut. The mode lands in normal whichever way the wrap was
    /// armed, an unknown pair key included
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    fn wrap_span(
        &mut self,
        span: Range<usize>,
        character: &str,
        record: bool,
        view: &View,
    ) -> Outcome {
        self.reset();
        self.mode = Mode::Normal;
        let Some((open, close, padded)) = surround_pair(character) else {
            return Outcome::Swallow;
        };
        let pad = if padded { " " } else { "" };
        let inner = view.text.get(span.clone()).unwrap_or_default();
        let text = format!("{open}{pad}{inner}{pad}{close}");
        // only ys left a Change::Wrap open for this key; a visual S
        // recorded none, and the dot after it replays whatever change came
        // before, deliberately
        // (adr/2026-08-surround-pair-set-and-padding.md)
        if let (true, Some(Change::Wrap { pair, .. })) =
            (record, &mut self.last_change)
        {
            *pair = Some(character.to_string());
        }
        let caret = span.start;
        Outcome::Acts(vec![Act::Checkpoint, Act::Splice { span, text, caret }])
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
        let wrapping = self.wrapping;
        self.reset();
        // ys named its noun's span; wait for the pair key instead of
        // cutting anything (adr/2026-08-surround-pair-set-and-padding.md)
        if wrapping {
            self.prefix = Some(Prefix::Wrap { span, record: true });
            return Outcome::Swallow;
        }
        let cut = view.text.get(span.clone()).unwrap_or_default();
        let mut yanked = cut.to_string();
        if linewise && !yanked.ends_with('\n') {
            // the clipboard is the one register: the trailing newline is
            // how linewise-ness survives the OS round trip
            // (adr/2026-08-one-register-the-clipboard.md)
            yanked.push('\n');
        }
        let mut acts = Vec::new();
        // a change intent begins here — yank changes nothing and
        // checkpoints nothing (adr/2026-08-undo-at-vim-grain.md)
        if op != Operator::Yank {
            acts.push(Act::Checkpoint);
        }
        // a charwise nothing yanks nothing; a linewise nothing is still a
        // line and its newline reaches the register, as vim's dd does — and
        // a recasing verb moves no text at all, so it fills no register
        // (adr/2026-08-case-operators-are-verbs.md)
        if !yanked.is_empty() && !op.recasing() {
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
                let (span, caret) = if linewise {
                    linewise_delete(view.text, span)
                } else {
                    let caret = charwise_delete_caret(
                        view.text,
                        &Lines::of(view.text, view.blocks).around(span.start),
                        &span,
                    );
                    (span, caret)
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
                // the session's typed text starts here, for the dot
                self.insert_from = Some(caret);
                acts.push(Act::Splice {
                    span,
                    text: String::new(),
                    caret,
                });
            }
            // the same landing rule the yank uses, for the same reason:
            // nothing moved, so a linewise recasing leaves the caret alone
            // and a charwise one owns its span's start
            // (adr/2026-08-case-operators-are-verbs.md)
            Operator::Lower | Operator::Upper | Operator::Flip => {
                // "alone" means on the same character: Ⱥ is two bytes and
                // ⱥ three, so the offset is measured in the recased text
                // (found by the key-sequence property)
                let caret = if linewise {
                    let prefix = cut
                        .get(..view.head.saturating_sub(span.start))
                        .unwrap_or_default();
                    span.start + recase(prefix, op).len()
                } else {
                    span.start
                };
                acts.push(Act::Splice {
                    text: recase(cut, op),
                    span,
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
        self.record(Change::Cut {
            forward,
            count: total,
        });
        Outcome::Acts(vec![
            Act::Checkpoint,
            Act::SetClipboard(cut),
            Act::Splice {
                span,
                text: String::new(),
                caret,
            },
        ])
    }

    /// s: [count] clusters at the caret become the insert session that
    /// follows — vim's cl, spelled as itself rather than recorded as
    /// `Noun::Motion(Motion::Right)`, whose landing clamps at the line's
    /// *last cluster*: standing on that cluster, the motion lands where it
    /// started and names an empty span, so the session would open in front
    /// of the cluster instead of eating it. `s` clamps at `line.end`
    /// instead, which is why `Noun::Clusters` is its alone
    /// (adr/2026-08-clusters-noun-belongs-to-s.md).
    fn change_clusters(
        &mut self,
        view: &View,
        line: &Range<usize>,
    ) -> Outcome {
        let total = self.count.max(1) as usize;
        let end = (0..total).fold(view.head, |from, _| {
            caret::next_cluster(view.text, from).min(line.end)
        });
        self.record(Change::Operate {
            op: Operator::Change,
            noun: Noun::Clusters,
            count: total,
            typed: String::new(),
        });
        self.finish_operator(Operator::Change, view.head..end, false, view)
    }

    /// R: opens the overwrite session — one checkpoint, no count
    /// (adr/2026-08-replace-mode-session-and-backspace.md).
    fn begin_replace(&mut self, view: &View) -> Outcome {
        self.reset();
        self.mode = Mode::Replace;
        self.replace_from = Some(view.head);
        self.replaced.clear();
        self.record(Change::Overwrite {
            typed: String::new(),
        });
        Outcome::Acts(vec![Act::Checkpoint])
    }

    /// r: [count] clusters become [count] copies of the character; fewer
    /// on the line than the count and the whole thing fails, as vim's does.
    fn replace_clusters(&mut self, character: &str, view: &View) -> Outcome {
        let total = self.count.max(1) as usize;
        let visual = match self.mode {
            Mode::Visual(kind) => Some(kind),
            _ => None,
        };
        self.reset();
        let Some(wanted) = character.chars().next() else {
            return Outcome::Swallow;
        };
        // visual r overwrites the whole selection, count and all ignored;
        // the newlines inside it stay newlines, or the lines would merge
        // (adr/2026-08-visual-gains-p-r-s-and-gv.md)
        if let Some(kind) = visual {
            return self.replace_selection(kind, wanted, view);
        }
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
        self.record(Change::Replace {
            ch: wanted,
            count: total,
        });
        Outcome::Acts(vec![
            Act::Checkpoint,
            Act::Splice {
                span: view.head..end,
                text,
                caret,
            },
        ])
    }

    /// ~: flip the case of [count] clusters and step past them.
    /// J and gJ: the caret's row and the ones below it become one line.
    /// The substrate is the visible-line table, never the raw newlines —
    /// the bytes between two rows may be a block separator, and replacing
    /// them collapses the two blocks exactly as a cross-block operator span
    /// already does (adr/2026-08-join-walks-the-visible-rows.md).
    fn join(&mut self, spaced: bool, view: &View) -> Outcome {
        // vim's J takes two lines at count 1 and at count 2 alike
        let total = self.effective_count().max(2);
        self.reset();
        let lines = Lines::of(view.text, view.blocks);
        let first = lines.row_of(view.head);
        let last = (first + total - 1).min(lines.rows() - 1);
        if last == first {
            // the note's last line has nothing below to pull up
            return Outcome::Swallow;
        }
        let head = lines.row(first);
        let span = head.start..lines.row(last).end;
        let joined = (first..=last).fold(String::new(), |mut out, row| {
            let slice = view.text.get(lines.row(row)).unwrap_or_default();
            if row == first {
                out.push_str(slice);
                return out;
            }
            // gJ takes the next line exactly as it stands, indent included
            let tail = if spaced { slice.trim_start() } else { slice };
            if spaced && joins_with_space(&out, tail) {
                out.push(' ');
            }
            out.push_str(tail);
            out
        });
        // the caret rests where the join happened — on the space it
        // inserted — and clamps back onto a cluster when the pulled-up
        // line was empty and there is nothing at the boundary to rest on
        let boundary = head.end;
        let end = span.start + joined.len();
        let caret = if boundary >= end && !joined.is_empty() {
            span.start + caret::prev_cluster(&joined, joined.len())
        } else {
            boundary
        };
        self.record(Change::Join {
            spaced,
            count: total,
        });
        Outcome::Acts(vec![
            Act::Checkpoint,
            Act::Splice {
                span,
                text: joined,
                caret,
            },
        ])
    }

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
        let flipped = recase(
            view.text.get(span.clone()).unwrap_or_default(),
            Operator::Flip,
        );
        // the landing is measured in the flipped text, whose bytes may
        // not match the span's (Ⱥ is two, ⱥ three): past the flipped run,
        // or on the line's last cluster when the run reached its end
        let caret = if end >= line.end {
            let last = unicode_segmentation::UnicodeSegmentation::graphemes(
                flipped.as_str(),
                true,
            )
            .next_back()
            .map_or(0, str::len);
            span.start + flipped.len() - last
        } else {
            span.start + flipped.len()
        };
        self.record(Change::Toggle { count: total });
        Outcome::Acts(vec![
            Act::Checkpoint,
            Act::Splice {
                span,
                text: flipped,
                caret,
            },
        ])
    }

    fn paste(&mut self, before: bool) -> Outcome {
        let total = self.count.max(1) as usize;
        self.reset();
        self.record(Change::Paste {
            before,
            count: total,
        });
        Outcome::Acts(vec![
            Act::Checkpoint,
            Act::Paste {
                before,
                count: total,
            },
        ])
    }

    /// One insert entry: the landing acts, the checkpoint, the recording
    /// for the dot, and where the session's typing will begin.
    fn begin_insert(
        &mut self,
        entry: InsertEntry,
        view: &View,
        line: &Range<usize>,
    ) -> Outcome {
        self.mode = Mode::Insert;
        self.reset();
        let (mut acts, from) = entry_acts(view, line, entry);
        acts.insert(0, Act::Checkpoint);
        self.insert_from = Some(from);
        self.last_change = Some(Change::Insert {
            entry,
            typed: String::new(),
        });
        Outcome::Acts(acts)
    }

    /// n and N: the committed pattern's next occurrence, wrap-around; the
    /// landing activates whichever block holds it
    /// (adr/2026-08-search-lands-through-place.md).
    /// Visual r: every cluster of the selection becomes the character,
    /// newlines excepted — they keep the lines apart. The selection is
    /// spent and the caret lands on its start, as vim leaves it.
    fn replace_selection(
        &mut self,
        kind: VisualKind,
        wanted: char,
        view: &View,
    ) -> Outcome {
        self.mode = Mode::Normal;
        let (span, _) = visual_span(kind, view);
        let text: String = view
            .text
            .get(span.clone())
            .unwrap_or_default()
            .chars()
            .map(|ch| if ch == '\n' { '\n' } else { wanted })
            .collect();
        if text.is_empty() {
            return Outcome::Acts(vec![Act::Place(span.start)]);
        }
        Outcome::Acts(vec![
            Act::Checkpoint,
            Act::Splice {
                caret: span.start,
                span,
                text,
            },
        ])
    }

    fn search_jump(&mut self, forward: bool, view: &View) -> Outcome {
        self.reset();
        self.jump_to_search(forward, view)
    }

    /// The word under the caret becomes the pattern — whole-word, which is
    /// the whole point of `*` — and the caret walks to the next or previous
    /// one. `n` and `N` then keep walking that same kind of hit
    /// (adr/2026-08-star-searches-whole-words.md).
    fn search_word(&mut self, forward: bool, view: &View) -> Outcome {
        self.reset();
        let lines = Lines::of(view.text, view.blocks);
        let Some(span) = motions::word_at_or_after(
            view.text,
            &lines.around(view.head),
            view.head,
        ) else {
            // no word left on this line: nothing to search for
            return Outcome::Swallow;
        };
        self.search = Some(Pattern {
            text: view.text.get(span).unwrap_or_default().to_string(),
            whole_word: true,
        });
        self.jump_to_search(forward, view)
    }

    fn jump_to_search(&mut self, forward: bool, view: &View) -> Outcome {
        let Some(pattern) = self.search.clone() else {
            return Outcome::Swallow;
        };
        let lines = Lines::of(view.text, view.blocks);
        match motions::search(view.text, &lines, view.head, &pattern, forward)
        {
            Some(hit) => Outcome::Acts(vec![Act::Place(hit)]),
            None => Outcome::Swallow,
        }
    }

    /// One submitted `:` line. Pure over the same `View` every keystroke
    /// sees, so the whole ex vocabulary tests headlessly
    /// (adr/2026-08-ex-line-is-literal-and-global.md).
    pub fn commit_ex(&mut self, line: &str, view: &View) -> ExOutcome {
        let line = line.trim();
        let (range, rest) = split_range(line);
        match rest {
            // a bare Enter closes the prompt and does nothing, as vim's does
            "" => self.ex_place(range, line, view),
            "w" => ExOutcome::Acts(vec![Act::Save]),
            "s" => ExOutcome::Refused(
                "\":s\" needs a pattern — spell it :s/old/new/".to_string(),
            ),
            _ => match rest.strip_prefix('s').map(str::chars) {
                // vim's own rule: the delimiter is whatever follows the s,
                // and it may never be a letter or a digit — :sort would
                // otherwise read its own o as one
                Some(mut body) => match body.next() {
                    Some(delim) if !delim.is_alphanumeric() => {
                        self.substitute(range, delim, body.as_str(), view)
                    }
                    _ => ExOutcome::Refused(unknown_command(line)),
                },
                None => ExOutcome::Refused(unknown_command(line)),
            },
        }
    }

    /// `:N` — the only command spelled entirely as its range.
    fn ex_place(
        &mut self,
        range: ExRange,
        spelled: &str,
        view: &View,
    ) -> ExOutcome {
        let lines = Lines::of(view.text, view.blocks);
        match range {
            ExRange::Line => ExOutcome::Acts(Vec::new()),
            ExRange::Rows(row, _) => {
                let rows = lines.rows();
                if row < 1 || row > rows {
                    return ExOutcome::Refused(format!(
                        "\":{spelled}\" is past the note's {rows} lines — \
                         the caret stayed"
                    ));
                }
                let target = lines.row(row - 1);
                ExOutcome::Acts(vec![Act::Place(motions::first_non_blank(
                    view.text, &target,
                ))])
            }
            ExRange::Whole | ExRange::Selection => {
                ExOutcome::Refused(format!(
                    "\":{spelled}\" names a range with no command — \
                     add s/old/new/ after it"
                ))
            }
        }
    }

    /// `:[range]s/old/new/`. Literal substrings, never regex — no regex
    /// crate enters for this, and reading Typst prose with one is what the
    /// never-regex rule already forbids. Every occurrence on every row in
    /// range goes, always: `gdefault` is how the user's own vim is
    /// configured, so a trailing `g` is accepted and ignored
    /// (adr/2026-08-ex-line-is-literal-and-global.md).
    fn substitute(
        &mut self,
        range: ExRange,
        delim: char,
        body: &str,
        view: &View,
    ) -> ExOutcome {
        // an unterminated :s/old is vim's "replace with nothing"
        let (pattern, tail) = body.split_once(delim).unwrap_or((body, ""));
        let (replacement, _flags) =
            tail.split_once(delim).unwrap_or((tail, ""));
        if pattern.is_empty() {
            return ExOutcome::Refused(
                "\":s\" needs a pattern — spell it :s/old/new/".to_string(),
            );
        }
        let lines = Lines::of(view.text, view.blocks);
        let (first, last) = range.rows(&lines, view);
        let span = lines.row(first).start..lines.row(last).end;
        let smart = motions::smartcase(pattern);
        let mut out = String::new();
        let mut caret = span.start;
        let mut hits = 0;
        let mut seam = span.start;
        for row in first..=last {
            let line = lines.row(row);
            // the bytes between two rows — a newline, or a whole block
            // separator — are nobody's line and ride along untouched
            out.push_str(view.text.get(seam..line.start).unwrap_or_default());
            let opened = out.len();
            let (replaced, found) = replace_all(
                view.text.get(line.clone()).unwrap_or_default(),
                pattern,
                replacement,
                smart,
            );
            if found > 0 {
                // vim leaves the caret on the last row it touched
                caret = span.start + opened;
                hits += found;
            }
            out.push_str(&replaced);
            seam = line.end;
        }
        if hits == 0 {
            return ExOutcome::Refused(format!(
                "\":s\" found no \"{pattern}\" in the range — widen it with \
                 :%s, or check the case"
            ));
        }
        // one splice, so one u takes the whole substitution back (AIR ACT-2)
        ExOutcome::Acts(vec![
            Act::Checkpoint,
            Act::Splice {
                span,
                text: out,
                caret,
            },
        ])
    }

    /// A fresh note begins thinking: normal mode, the pending grammar
    /// cleared — while the dot, the pattern and the find survive across
    /// notes, as vim's registers do
    /// (adr/2026-08-escape-ladder-editor-wide-mode.md).
    pub fn note_opened(&mut self) {
        self.mode = Mode::Normal;
        self.insert_from = None;
        self.replace_from = None;
        self.replaced.clear();
        self.reset();
    }

    /// Whether a count is still accumulating — the `2` of `2j`, which the
    /// grammar swallows. The executor asks before forgetting a j/k run's
    /// goal column: vim's curswant survives a count, so `j` then `2j`
    /// keeps walking the column the first `j` resolved instead of
    /// bootstrapping a fresh one
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    pub fn counting(&self) -> bool {
        self.count > 0 || self.count2 > 0
    }

    /// The prompt's Enter: the pattern commits, and the first jump is the
    /// n the widget synthesizes right after.
    pub fn commit_search(&mut self, pattern: String) {
        if !pattern.is_empty() {
            self.search = Some(Pattern::loose(pattern));
        }
    }

    /// . — the recorded change, resolved again at the caret it finds; a
    /// count ahead of the dot overrides the recorded one, as vim's does.
    fn repeat(&mut self, view: &View) -> Outcome {
        let Some(change) = self.last_change.clone() else {
            self.reset();
            return Outcome::Swallow;
        };
        let over = (self.count > 0).then_some(self.count as usize);
        self.count = 0;
        let line = Lines::of(view.text, view.blocks).around(view.head);
        match change {
            Change::Operate {
                op,
                noun,
                count,
                typed,
            } => self.repeat_operate(
                op,
                noun,
                over.unwrap_or(count),
                typed,
                view,
            ),
            Change::Cut { forward, count } => {
                self.count = over.unwrap_or(count) as u32;
                self.cut_clusters(view, &line, forward)
            }
            Change::Replace { ch, count } => {
                self.count = over.unwrap_or(count) as u32;
                self.replace_clusters(&ch.to_string(), view)
            }
            Change::Toggle { count } => {
                self.count = over.unwrap_or(count) as u32;
                self.toggle_case(view, &line)
            }
            Change::Join { spaced, count } => {
                self.count = over.unwrap_or(count) as u32;
                self.join(spaced, view)
            }
            Change::Paste { before, count } => Outcome::Acts(vec![
                Act::Checkpoint,
                Act::Paste {
                    before,
                    count: over.unwrap_or(count),
                },
            ]),
            Change::Insert { entry, typed } => {
                self.replay_insert(entry, &typed, view, &line)
            }
            Change::Overwrite { typed } => self.repeat_overwrite(&typed, view),
            Change::Resurround { old, new } => {
                self.apply_surround(old, new, view)
            }
            Change::Wrap { noun, count, pair } => {
                self.repeat_wrap(noun, over.unwrap_or(count), pair, view)
            }
        }
    }

    /// Replaying an operator: motions and lines re-resolve through their
    /// own paths; a change-verb replay applies its recorded text and stays
    /// in normal — the dot never opens an insert session.
    fn repeat_operate(
        &mut self,
        op: Operator,
        noun: Noun,
        count: usize,
        typed: String,
        view: &View,
    ) -> Outcome {
        let outcome = self.resolve_noun(op, noun, count, view);
        if op != Operator::Change {
            return outcome;
        }
        self.mode = Mode::Normal;
        self.insert_from = None;
        // re-record wholesale so the typed text survives for the next dot
        // whatever the noun's re-resolution did
        self.record(Change::Operate {
            op,
            noun,
            count,
            typed: typed.clone(),
        });
        let Outcome::Acts(mut acts) = outcome else {
            return outcome;
        };
        let landing = acts.iter().find_map(|act| match act {
            Act::Splice { caret, .. } => Some(*caret),
            _ => None,
        });
        if let (Some(at), false) = (landing, typed.is_empty()) {
            let back = caret::prev_cluster(&typed, typed.len());
            acts.push(Act::Type(typed));
            acts.push(Act::Place(at + back));
        }
        Outcome::Acts(acts)
    }

    /// A noun re-resolved through its own grammar path: a motion or the
    /// current lines re-run their own machinery, which records its own
    /// dot state as it goes; an object re-finds its span through
    /// `object_noun`. Shared by the dot's operator replay above and ys's
    /// replay below — both just re-run whatever produced the noun the
    /// first time, under a different `op`.
    fn resolve_noun(
        &mut self,
        op: Operator,
        noun: Noun,
        count: usize,
        view: &View,
    ) -> Outcome {
        match noun {
            Noun::Motion(motion) => {
                self.count2 = count as u32;
                self.operator = Some(op);
                self.operator_motion(op, motion, view)
            }
            Noun::Lines => {
                self.count = count as u32;
                self.current_lines(op, view)
            }
            Noun::Clusters => {
                self.count = count as u32;
                let line = Lines::of(view.text, view.blocks).around(view.head);
                self.change_clusters(view, &line)
            }
            Noun::Object { kind, around } => {
                self.object_noun(op, kind, around, view)
            }
        }
    }

    /// The dot on a ys: the noun re-resolves through `resolve_noun`
    /// exactly as a verb's replay does, `wrapping` and the operator armed
    /// the same way `s` arms them live; the pair key recorded the first
    /// time then feeds straight into `wrap_span`
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    fn repeat_wrap(
        &mut self,
        noun: WrapNoun,
        count: usize,
        pair: Option<String>,
        view: &View,
    ) -> Outcome {
        let Some(key) = pair else {
            self.reset();
            return Outcome::Swallow;
        };
        self.wrapping = true;
        self.operator = Some(Operator::Yank);
        self.resolve_noun(Operator::Yank, noun.into(), count, view);
        let Some(Prefix::Wrap { span, .. }) = self.prefix.take() else {
            self.reset();
            return Outcome::Swallow;
        };
        self.wrap_span(span, &key, true, view)
    }

    /// Replaying an insert entry: the same landing, the recorded text,
    /// the caret resting on its last cluster.
    fn replay_insert(
        &mut self,
        entry: InsertEntry,
        typed: &str,
        view: &View,
        line: &Range<usize>,
    ) -> Outcome {
        self.reset();
        let (mut acts, from) = entry_acts(view, line, entry);
        acts.insert(0, Act::Checkpoint);
        if !typed.is_empty() {
            acts.push(Act::Type(typed.to_string()));
            acts.push(Act::Place(
                from + caret::prev_cluster(typed, typed.len()),
            ));
        }
        Outcome::Acts(acts)
    }

    /// The dot on an R session: the whole recorded overwrite replays as
    /// one splice, never a keystroke-by-keystroke walk — and the mode
    /// stays Normal, since the dot never opens a session
    /// (adr/2026-08-replace-mode-session-and-backspace.md).
    fn repeat_overwrite(&mut self, typed: &str, view: &View) -> Outcome {
        self.reset();
        if typed.is_empty() {
            return Outcome::Swallow;
        }
        let line = Lines::of(view.text, view.blocks).around(view.head);
        let count =
            unicode_segmentation::UnicodeSegmentation::graphemes(typed, true)
                .count();
        let end = (0..count).fold(view.head, |from, _| {
            caret::next_cluster(view.text, from).min(line.end)
        });
        let overwritten = view
            .text
            .get(view.head..end)
            .unwrap_or_default()
            .to_string();
        let caret = view.head + caret::prev_cluster(typed, typed.len());
        self.record(Change::Overwrite {
            typed: typed.to_string(),
        });
        let mut acts = vec![Act::Checkpoint];
        if !overwritten.is_empty() {
            acts.push(Act::SetClipboard(overwritten));
        }
        acts.push(Act::Splice {
            span: view.head..end,
            text: typed.to_string(),
            caret,
        });
        Outcome::Acts(acts)
    }

    fn record(&mut self, change: Change) {
        self.last_change = Some(change);
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
        self.wrapping = false;
        self.prefix = None;
        self.goal = None;
    }
}

/// A line's new leading-space count: one level in or out, and a blank line
/// left exactly as blank as vim's own > leaves it.
fn indented(body: &str, spaces: usize, deeper: bool) -> usize {
    if body.trim().is_empty() {
        spaces
    } else if deeper {
        spaces + caret::INDENT.len()
    } else {
        spaces.saturating_sub(caret::INDENT.len())
    }
}

/// The selection's span and its line-wise-ness, as every verb over a
/// selection sees it — char-wise takes both end clusters, as vim's visual
/// does; line-wise takes the whole lines. Shared so the wrap S arms and
/// the cut a verb makes can never drift apart.
/// Putting a span *into* the selection rather than handing it to a verb:
/// the anchor takes one end, the head the other, and the caret rests on
/// the span's last cluster the way visual mode always draws its head
/// (adr/2026-08-objects-reach-visual-mode.md).
fn select_span(span: Range<usize>, kind: VisualKind, view: &View) -> Vec<Act> {
    let head = match kind {
        // a line-wise selection redraws itself from its ends' rows, so the
        // raw end is enough; a char-wise one rests on the last cluster
        VisualKind::Line => span.end,
        VisualKind::Char => {
            caret::prev_cluster(view.text, span.end).max(span.start)
        }
    };
    vec![Act::Place(span.start), Act::Extend(head)]
}

fn visual_span(kind: VisualKind, view: &View) -> (Range<usize>, bool) {
    let low = view.head.min(view.anchor);
    let high = view.head.max(view.anchor);
    match kind {
        VisualKind::Char => (low..caret::next_cluster(view.text, high), false),
        VisualKind::Line => {
            let lines = Lines::of(view.text, view.blocks);
            let span = motions::linewise_span(
                &lines,
                lines.row_of(low),
                lines.row_of(high),
            );
            (span, true)
        }
    }
}

/// The kind table adr/2026-08-motions-on-visible-lines.md recorded: e \$
/// and the forward finds are inclusive, the verticals and whole-note jumps
/// linewise, everything else exclusive — ; and , arrive here already
/// normalized into the find they repeat.
impl ExRange {
    /// The row window this range names, low to high and clamped onto the
    /// table.
    fn rows(self, lines: &Lines, view: &View) -> (usize, usize) {
        let top = lines.rows() - 1;
        let (first, last) = match self {
            ExRange::Line => {
                let row = lines.row_of(view.head);
                (row, row)
            }
            ExRange::Whole => (0, top),
            ExRange::Selection => (
                lines.row_of(view.head.min(view.anchor)),
                lines.row_of(view.head.max(view.anchor)),
            ),
            // spelled one-based, as vim counts its lines
            ExRange::Rows(first, last) => (
                first.saturating_sub(1).min(top),
                last.saturating_sub(1).min(top),
            ),
        };
        (first.min(last), first.max(last))
    }
}

/// The range a `:` line opens with, and whatever command follows it.
fn split_range(line: &str) -> (ExRange, &str) {
    if let Some(rest) = line.strip_prefix('%') {
        return (ExRange::Whole, rest.trim_start());
    }
    if let Some(rest) = line.strip_prefix("'<,'>") {
        return (ExRange::Selection, rest.trim_start());
    }
    let (Some(first), rest) = take_number(line) else {
        return (ExRange::Line, line);
    };
    let Some(after) = rest.strip_prefix(',') else {
        return (ExRange::Rows(first, first), rest.trim_start());
    };
    let (second, rest) = take_number(after);
    (
        ExRange::Rows(first, second.unwrap_or(first)),
        rest.trim_start(),
    )
}

/// The digits a range opens with, if any, and what is left after them.
fn take_number(line: &str) -> (Option<usize>, &str) {
    let digits = line
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(line.len());
    (line[..digits].parse().ok(), &line[digits..])
}

/// Every occurrence of the literal pattern on one row, left to right and
/// non-overlapping, and how many there were
/// (adr/2026-08-ex-line-is-literal-and-global.md).
fn replace_all(
    slice: &str,
    pattern: &str,
    replacement: &str,
    smart: bool,
) -> (String, usize) {
    let mut out = String::new();
    let mut kept = 0;
    let mut hits = 0;
    for (offset, _) in slice.char_indices() {
        // a hit consumed the bytes up to `kept`; nothing inside one can
        // start another
        if offset < kept {
            continue;
        }
        let Some(length) = motions::match_len(slice, offset, pattern, smart)
        else {
            continue;
        };
        out.push_str(slice.get(kept..offset).unwrap_or_default());
        out.push_str(replacement);
        kept = offset + length;
        hits += 1;
    }
    out.push_str(slice.get(kept..).unwrap_or_default());
    (out, hits)
}

/// What the status surface says about a `:` line it could not read: what
/// happened, and what to type instead (AIR ERR-1).
fn unknown_command(line: &str) -> String {
    format!(
        "unknown command \":{line}\" — :w saves, :s/old/new/ substitutes, \
         :12 goes to a line"
    )
}

/// The three case verbs over one span. `~` on a single cluster is the same
/// arithmetic as `g~` over a motion, so both go through here
/// (adr/2026-08-case-operators-are-verbs.md).
fn recase(text: &str, op: Operator) -> String {
    text.chars()
        .flat_map(|ch| match op {
            Operator::Upper => ch.to_uppercase().collect::<Vec<char>>(),
            Operator::Lower => ch.to_lowercase().collect(),
            _ if ch.is_uppercase() => ch.to_lowercase().collect(),
            _ => ch.to_uppercase().collect(),
        })
        .collect()
}

/// vim's rules for the space J puts between two lines: none when there is
/// nothing on either side of the join, when the first line already ends in
/// whitespace, or when the second opens with a closing bracket
/// (adr/2026-08-join-walks-the-visible-rows.md).
fn joins_with_space(before: &str, after: &str) -> bool {
    !before.is_empty()
        && !before.ends_with(char::is_whitespace)
        && !after.is_empty()
        && !after.starts_with(')')
}

fn span_kind(motion: Motion) -> SpanKind {
    match motion {
        Motion::Down | Motion::Up | Motion::FirstLine | Motion::LastLine => {
            SpanKind::Linewise
        }
        Motion::WordEnd
        | Motion::BigWordEnd
        | Motion::WordEndBack
        | Motion::BigWordEndBack
        | Motion::MatchPair
        | Motion::LineEnd => SpanKind::Inclusive,
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

/// A linewise delete always takes a newline with it: forward when the span
/// already swallowed a following line's own separator, backward when the
/// deleted run reaches the note's very end and a line precedes it — vim
/// never leaves a blank line behind just because the deletion reached the
/// note's end. A span that stops short of a following separator for its
/// own reasons (`ip`, which never claims the block's separator) is left
/// alone unless it too runs to the note's end, so this backward pull never
/// fires on an object span that merely chose not to reach forward. The
/// only line in the note has no newline either side, so it is emptied
/// rather than removed. Returns the span to splice and the caret's landing
/// — the first non-blank of whichever line slid into the gap.
fn linewise_delete(text: &str, span: Range<usize>) -> (Range<usize>, usize) {
    let sliced = text.get(span.clone()).unwrap_or_default();
    if sliced.ends_with('\n') {
        let following = text.get(span.end..).unwrap_or_default();
        (span.clone(), span.start + motions::blank_prefix(following))
    } else if span.end == text.len()
        && text
            .get(..span.start)
            .is_some_and(|before| before.ends_with('\n'))
    {
        let prev = text[..span.start - 1].rfind('\n').map_or(0, |i| i + 1);
        let caret = prev + motions::blank_prefix(&text[prev..span.start - 1]);
        (span.start - 1..span.end, caret)
    } else {
        (span.clone(), span.start)
    }
}

/// One insert entry's landing: the acts that place the caret, and the
/// offset where the session's typing begins.
fn entry_acts(
    view: &View,
    line: &Range<usize>,
    entry: InsertEntry,
) -> (Vec<Act>, usize) {
    match entry {
        InsertEntry::Before => (vec![], view.head),
        // append: after the cluster under the caret, never past the line
        InsertEntry::After => {
            let target =
                caret::next_cluster(view.text, view.head).min(line.end);
            (vec![Act::Place(target)], target)
        }
        InsertEntry::FirstNonBlank => {
            let target = motions::first_non_blank(view.text, line);
            (vec![Act::Place(target)], target)
        }
        InsertEntry::LineEnd => (vec![Act::Place(line.end)], line.end),
        // open a line below: a newline at the line's end, the caret
        // riding past it onto the fresh line
        InsertEntry::Below => (
            vec![Act::Place(line.end), Act::Type("\n".to_string())],
            line.end + 1,
        ),
        // open a line above: a newline at the line's start, the caret
        // stepping back onto the fresh line before it
        InsertEntry::Above => (
            vec![
                Act::Place(line.start),
                Act::Type("\n".to_string()),
                Act::Place(line.start),
            ],
            line.start,
        ),
    }
}

/// The nouns: word, the quote flavours, the bracket pairs — French
/// guillemets included — and the block-as-paragraph.
fn object_kind(character: &str) -> Option<ObjectKind> {
    match character {
        "w" => Some(ObjectKind::Word),
        "W" => Some(ObjectKind::BigWord),
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

/// The surround targets — the guillemets among them, so a French vault's
/// own quotes answer ds and cs exactly as the text objects already let di
/// name them (adr/2026-08-surround-pair-set-and-padding.md).
fn surround_kind(character: &str) -> Option<ObjectKind> {
    match character {
        "*" => Some(ObjectKind::Quote('*')),
        "_" => Some(ObjectKind::Quote('_')),
        "'" => Some(ObjectKind::Quote('\'')),
        "\"" => Some(ObjectKind::Quote('"')),
        "`" => Some(ObjectKind::Quote('`')),
        "(" | ")" | "b" => Some(ObjectKind::Pair('(', ')')),
        "[" | "]" => Some(ObjectKind::Pair('[', ']')),
        "{" | "}" | "B" => Some(ObjectKind::Pair('{', '}')),
        "<" | ">" => Some(ObjectKind::Pair('<', '>')),
        "«" | "»" => Some(ObjectKind::Pair('«', '»')),
        _ => None,
    }
}

/// The pair a surround key writes, and whether wrapping pads it — only
/// the opening bracket keys pad; the closing keys, b, B and every
/// quote-like key are bare. « pads like the opening bracket it is, which
/// is also French typography's own spacing, and » writes the pair bare
/// (adr/2026-08-surround-pair-set-and-padding.md).
fn surround_pair(character: &str) -> Option<(String, String, bool)> {
    match character {
        "*" => Some(("*".to_string(), "*".to_string(), false)),
        "_" => Some(("_".to_string(), "_".to_string(), false)),
        "'" => Some(("'".to_string(), "'".to_string(), false)),
        "\"" => Some(("\"".to_string(), "\"".to_string(), false)),
        "`" => Some(("`".to_string(), "`".to_string(), false)),
        "(" => Some(("(".to_string(), ")".to_string(), true)),
        ")" | "b" => Some(("(".to_string(), ")".to_string(), false)),
        "[" => Some(("[".to_string(), "]".to_string(), true)),
        "]" => Some(("[".to_string(), "]".to_string(), false)),
        "{" => Some(("{".to_string(), "}".to_string(), true)),
        "}" | "B" => Some(("{".to_string(), "}".to_string(), false)),
        "<" => Some(("<".to_string(), ">".to_string(), true)),
        ">" => Some(("<".to_string(), ">".to_string(), false)),
        "«" => Some(("«".to_string(), "»".to_string(), true)),
        "»" => Some(("«".to_string(), "»".to_string(), false)),
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

    fn insert() -> Vim {
        Vim {
            mode: Mode::Insert,
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

    /// Feeds arbitrary keys, the modifier keydowns included — what the
    /// webview really sends before a shifted character, and what `feed`'s
    /// string form cannot spell.
    fn feed_keys(vim: &mut Vim, keys: &[Key], view: &View) -> Outcome {
        let mut last = Outcome::Swallow;
        for key in keys {
            last = vim.handle(key, Modifiers::empty(), view);
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
        assert_eq!(
            Vim::default().mode,
            Mode::Normal,
            "a new file starts thinking"
        );
        let mut vim = insert();
        for key in [
            character("x"),
            character("é"),
            Key::Enter,
            Key::Backspace,
            Key::ArrowLeft,
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
        let mut vim = insert();
        let outcome = vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(NOTE, &parsed, 9),
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(7)]));

        let mut vim = insert();
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
            stripped(vim.handle(&character("i"), Modifiers::empty(), &sight)),
            Outcome::Acts(vec![]),
            "i writes where the caret stands"
        );
        assert_eq!(vim.mode, Mode::Insert);

        let mut vim = normal();
        assert_eq!(
            stripped(vim.handle(&character("a"), Modifiers::empty(), &sight)),
            Outcome::Acts(vec![Act::Place(35)]),
            "a appends after the é"
        );

        let mut vim = normal();
        assert_eq!(
            stripped(vim.handle(&character("I"), Modifiers::empty(), &sight)),
            Outcome::Acts(vec![Act::Place(23)]),
            "I lands on the first non-blank"
        );

        let mut vim = normal();
        assert_eq!(
            stripped(vim.handle(&character("A"), Modifiers::empty(), &sight)),
            Outcome::Acts(vec![Act::Place(36)]),
            "A lands at the line's end"
        );

        let mut vim = normal();
        assert_eq!(
            stripped(vim.handle(&character("o"), Modifiers::empty(), &sight)),
            Outcome::Acts(vec![Act::Place(36), Act::Type("\n".to_string())]),
            "o opens below"
        );

        let mut vim = normal();
        assert_eq!(
            stripped(vim.handle(&character("O"), Modifiers::empty(), &sight)),
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
    fn escape_in_normal_mode_is_inert_and_shift_escape_renders_the_block() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        let mut vim = normal();
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::empty(), &sight),
            Outcome::Swallow,
            "a plain Escape never leaves the note"
        );
        assert_eq!(vim.mode, Mode::Normal, "and changes nothing");
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::SHIFT, &sight),
            Outcome::Acts(vec![Act::Deactivate]),
            "shift+Escape is the way out"
        );
        assert_eq!(vim.mode, Mode::Normal, "the mode survives the exit");
    }

    #[test]
    fn shift_escape_closes_the_mode_session_then_leaves_the_note() {
        let parsed = blocks::segment(NOTE);

        // insert: the caret steps back as a plain Escape leaves it, and
        // the dot keeps the session's typed text
        let mut vim = Vim {
            mode: Mode::Insert,
            insert_from: Some(7),
            last_change: Some(Change::Insert {
                entry: InsertEntry::Before,
                typed: String::new(),
            }),
            ..Vim::default()
        };
        assert_eq!(
            vim.handle(
                &Key::Escape,
                Modifiers::SHIFT,
                &view(NOTE, &parsed, 9)
            ),
            Outcome::Acts(vec![Act::Place(7), Act::Deactivate]),
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(
            vim.last_change,
            Some(Change::Insert {
                entry: InsertEntry::Before,
                typed: NOTE[7..9].to_string(),
            }),
        );

        // R: the session's overwritten run still reaches the one register
        let text = "XY été\n";
        let replaced = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(0),
            replaced: vec!["u".to_string(), "n".to_string()],
            last_change: Some(Change::Overwrite {
                typed: String::new(),
            }),
            ..Vim::default()
        };
        assert_eq!(
            vim.handle(
                &Key::Escape,
                Modifiers::SHIFT,
                &view(text, &replaced, 2)
            ),
            Outcome::Acts(vec![
                Act::SetClipboard("un".into()),
                Act::Place(1),
                Act::Deactivate,
            ]),
        );
        assert_eq!(vim.mode, Mode::Normal);

        // visual: the caret lands at the head before the block renders
        let mut vim = normal();
        feed(&mut vim, "v", &view(NOTE, &parsed, 2));
        assert_eq!(
            vim.handle(
                &Key::Escape,
                Modifiers::SHIFT,
                &spread(NOTE, &parsed, 2, 5)
            ),
            Outcome::Acts(vec![Act::Place(5), Act::Deactivate]),
        );
        assert_eq!(vim.mode, Mode::Normal);
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
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: false
            }]),
            "j after the arrow asks for one line, not three"
        );
    }

    #[test]
    fn motions_move_and_counts_compose() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: false
            }]),
            "j asks the executor to walk one visual line down"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "3j", &sight),
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 3,
                extend: false
            }]),
            "3j composes the count for the executor"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "12l", &sight),
            Outcome::Acts(vec![Act::Place(7)]),
            "12l clamps onto the heading's last cluster"
        );
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "h", &view(NOTE, &parsed, 7)),
            Outcome::Acts(vec![Act::Place(6)]),
            "h steps back a cluster"
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
            Outcome::Acts(vec![Act::Place(10)]),
            "[count]gg goes to the line"
        );
    }

    // plain j/k no longer touch this struct's logical goal column — they
    // ask the executor to walk visual lines instead — so the vertical
    // arrows, still resolved synchronously in visual mode, are the one
    // path left in the grammar that still remembers a column across a run
    // (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn the_goal_column_survives_arrow_runs_and_nothing_else() {
        let parsed = blocks::segment(NOTE);
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        let Outcome::Acts(first) = vim.handle(
            &Key::ArrowUp,
            Modifiers::empty(),
            &view(NOTE, &parsed, 33),
        ) else {
            panic!("arrow up extends")
        };
        assert_eq!(first, vec![Act::Extend(21)]);
        let Outcome::Acts(second) = vim.handle(
            &Key::ArrowUp,
            Modifiers::empty(),
            &view(NOTE, &parsed, 21),
        ) else {
            panic!("arrow up extends")
        };
        assert_eq!(
            second,
            vec![Act::Extend(10)],
            "the blank row, goal column held"
        );
        let Outcome::Acts(third) = vim.handle(
            &Key::ArrowUp,
            Modifiers::empty(),
            &view(NOTE, &parsed, 10),
        ) else {
            panic!("arrow up extends")
        };
        assert_eq!(third, vec![Act::Extend(7)], "the goal column held");
        vim.handle(
            &Key::ArrowLeft,
            Modifiers::empty(),
            &view(NOTE, &parsed, 7),
        );
        let Outcome::Acts(fourth) = vim.handle(
            &Key::ArrowUp,
            Modifiers::empty(),
            &view(NOTE, &parsed, 6),
        ) else {
            panic!("arrow up extends")
        };
        assert_ne!(fourth, vec![Act::Extend(21)], "the goal was forgotten");
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
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: false
            }]),
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
            vim.handle(&Key::Escape, Modifiers::SHIFT, &sight),
            Outcome::Acts(vec![Act::Deactivate]),
            "and the note is left with shift held, not without"
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

    /// The acts of one fed key string, panicking on anything but Acts —
    /// checkpoints stripped, since their emission has its own test.
    fn acts_of(keys: &str, text: &str, head: usize) -> Vec<Act> {
        let parsed = blocks::segment(text);
        let mut vim = normal();
        match feed(&mut vim, keys, &view(text, &parsed, head)) {
            Outcome::Acts(acts) => acts
                .into_iter()
                .filter(|act| *act != Act::Checkpoint)
                .collect(),
            other => panic!("{keys}: expected acts, got {other:?}"),
        }
    }

    /// An outcome with its checkpoints stripped, for the direct asserts.
    fn stripped(outcome: Outcome) -> Outcome {
        match outcome {
            Outcome::Acts(acts) => Outcome::Acts(
                acts.into_iter()
                    .filter(|act| *act != Act::Checkpoint)
                    .collect(),
            ),
            other => other,
        }
    }

    /// A dead key reaches the grammar as `Key::Dead` — or, when Dioxus
    /// cannot parse what the webview sent, as `Key::Unidentified`
    /// (`.unwrap_or(Key::Unidentified)` in its keyboard deserializer).
    /// Neither is a keystroke aimed at the grammar, and neither may cost
    /// a chord its verb: `^` is dead on a French layout, so `d^` used to
    /// arrive as d, a platform key, then the composed caret — and the
    /// platform key in the middle took the d with it.
    #[test]
    fn a_platform_key_does_not_abort_a_chord() {
        let text = "un café noir\nposé là\n";
        let parsed = blocks::segment(text);
        let sight = view(text, &parsed, 3);
        let cut = Outcome::Acts(vec![
            Act::SetClipboard("café noir".into()),
            Act::Splice {
                span: 3..13,
                text: String::new(),
                caret: 2,
            },
        ]);
        for stray in [Key::Dead, Key::Unidentified, Key::Fn] {
            let mut vim = normal();
            assert_eq!(
                stripped(feed_keys(
                    &mut vim,
                    &[character("d"), stray.clone(), character("$")],
                    &sight,
                )),
                cut,
                "{stray:?} between the verb and its motion",
            );
        }
        // a pending prefix waits through one too: d i <platform> " still
        // cuts inside the quotes
        let quoted = "un «café» noir\n";
        let parsed = blocks::segment(quoted);
        let mut vim = normal();
        assert_eq!(
            stripped(feed_keys(
                &mut vim,
                &[
                    character("d"),
                    character("i"),
                    Key::Unidentified,
                    character("«"),
                ],
                &view(quoted, &parsed, 5),
            )),
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 5..10,
                    text: String::new(),
                    caret: 5
                },
            ]),
        );
        // but an unbound *character* still aborts, as vim's does
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "dz", &sight),
            Outcome::Swallow,
            "dz is still nothing"
        );
        // and Escape still kills a pending prefix outright
        let mut vim = normal();
        assert_eq!(
            feed_keys(
                &mut vim,
                &[character("d"), character("i"), Key::Escape],
                &sight,
            ),
            Outcome::Swallow,
        );
        assert!(!vim.pending(), "Escape emptied the grammar");
    }

    /// The webview sends a modifier's own keydown before the character it
    /// modifies, so every chord whose continuation needs Shift or AltGr —
    /// d$, di", dF, c^ — used to lose its verb to normal mode's catch-all.
    #[test]
    fn a_modifier_keydown_leaves_the_pending_grammar_alone() {
        let text = "un café noir\nposé là\n";
        let parsed = blocks::segment(text);
        let sight = view(text, &parsed, 3);
        // d Shift $ cuts exactly what d$ cuts
        let mut vim = normal();
        assert_eq!(
            stripped(feed_keys(
                &mut vim,
                &[character("d"), Key::Shift, character("$")],
                &sight,
            )),
            Outcome::Acts(vec![
                Act::SetClipboard("café noir".into()),
                Act::Splice {
                    span: 3..13,
                    text: String::new(),
                    caret: 2
                },
            ]),
        );
        // a count outlives one too: 2 Shift w is 2w
        let mut vim = normal();
        assert_eq!(
            feed_keys(
                &mut vim,
                &[character("2"), Key::Shift, character("w")],
                &view(text, &parsed, 0),
            ),
            Outcome::Acts(vec![Act::Place(9)]),
        );
        // and a pending prefix survives: the object side, where the
        // modifier is not a Key::Character and used to abort the wait
        let quoted = "un «café» noir\n";
        let parsed = blocks::segment(quoted);
        let mut vim = normal();
        assert_eq!(
            stripped(feed_keys(
                &mut vim,
                &[
                    character("d"),
                    character("i"),
                    Key::AltGraph,
                    character("«"),
                ],
                &view(quoted, &parsed, 5),
            )),
            Outcome::Acts(vec![
                Act::SetClipboard("café".into()),
                Act::Splice {
                    span: 5..10,
                    text: String::new(),
                    caret: 5
                },
            ]),
        );
        // CapsLock is the third one a French writer trips over
        let mut vim = normal();
        assert_eq!(
            vim.handle(&Key::CapsLock, Modifiers::empty(), &sight),
            Outcome::Pass,
        );
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
        // dk mirrors dj going up: operator-pending k stays linewise over
        // logical lines too, the same two lines from the second one
        assert_eq!(acts_of("dk", text, 16), acts_of("dj", text, 3));
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
            stripped(outcome),
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
        // one undo step: a middle-line dd is still one Checkpoint, one Splice
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            checkpoint_and_splice_counts(&feed(
                &mut vim,
                "dd",
                &view(text, &parsed, 5)
            )),
            (1, 1),
            "one undo step"
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
            stripped(outcome),
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
        // the heading is now its own line-block, immediately followed by
        // a blank line-block: dd eats through the one newline separating
        // them (adr/2026-08-per-line-block-segmentation.md)
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        let outcome = feed(&mut vim, "dd", &view(NOTE, &parsed, 2));
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("= l'été\n".into()),
                Act::Splice {
                    span: 0..10,
                    text: String::new(),
                    caret: 0
                },
            ]),
            "the register still carries the linewise newline"
        );
    }

    /// How many checkpoints and splices one outcome carries — dd's one
    /// undo step is a `Checkpoint` and a `Splice`, never more, never less.
    fn checkpoint_and_splice_counts(outcome: &Outcome) -> (usize, usize) {
        let Outcome::Acts(acts) = outcome else {
            panic!("expected acts, got {outcome:?}");
        };
        (
            acts.iter().filter(|act| **act == Act::Checkpoint).count(),
            acts.iter()
                .filter(|act| matches!(act, Act::Splice { .. }))
                .count(),
        )
    }

    #[test]
    fn dd_on_the_notes_last_line_takes_the_preceding_newline() {
        // no following line to eat through, so the newline vim takes is
        // the one behind: the previous line slides up to become the last
        let text = "  une\ndeux";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "dd", &view(text, &parsed, 8));
        assert_eq!(
            checkpoint_and_splice_counts(&outcome),
            (1, 1),
            "one undo step"
        );
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("deux\n".into()),
                Act::Splice {
                    span: 5..10,
                    text: String::new(),
                    caret: 2
                },
            ]),
            "the preceding newline goes, caret on the line that slid up"
        );
    }

    #[test]
    fn dd_on_the_trailing_empty_line_removes_it() {
        // the caret sits on the note's real trailing empty line — the one
        // adr/2026-08-cursor-always-in-the-note.md keeps navigable — and
        // dd there must remove the line, not leave a second blank behind
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        let outcome = feed(&mut vim, "dd", &view(NOTE, &parsed, NOTE.len()));
        assert_eq!(
            checkpoint_and_splice_counts(&outcome),
            (1, 1),
            "one undo step"
        );
        let prose = NOTE.find("La pluie").expect("the prose line is in NOTE");
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("\n".into()),
                Act::Splice {
                    span: NOTE.len() - 1..NOTE.len(),
                    text: String::new(),
                    caret: prose,
                },
            ]),
            "the trailing newline goes, caret on the prose line above"
        );
    }

    #[test]
    fn dd_on_the_only_line_empties_it_without_removing_it() {
        // no line before or after: dd cannot take a newline from either
        // side, so the line is emptied in place rather than removed
        let text = "seule";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "dd", &view(text, &parsed, 3));
        assert_eq!(
            checkpoint_and_splice_counts(&outcome),
            (1, 1),
            "one undo step"
        );
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("seule\n".into()),
                Act::Splice {
                    span: 0..5,
                    text: String::new(),
                    caret: 0
                },
            ]),
        );
    }

    #[test]
    fn cc_on_the_last_line_keeps_its_line() {
        // unlike dd, a linewise change never takes a neighbouring newline —
        // there is always a line left to type into, last line included
        let text = "une\ndeux";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "cc", &view(text, &parsed, 5));
        assert_eq!(
            stripped(outcome),
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
            stripped(outcome),
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
    fn s_cuts_the_cluster_and_opens_insert() {
        let text = "un café\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "s", &view(text, &parsed, 0));
        let Outcome::Acts(acts) = outcome.clone() else {
            panic!("expected acts, got {outcome:?}");
        };
        assert_eq!(
            acts.first(),
            Some(&Act::Checkpoint),
            "s begins a change intent"
        );
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("u".into()),
                Act::Splice {
                    span: 0..1,
                    text: String::new(),
                    caret: 0
                },
            ]),
        );
        assert_eq!(vim.mode, Mode::Insert);
    }

    #[test]
    fn a_count_makes_s_eat_several_clusters() {
        let text = "un café\n";
        // 3s from the start: the three clusters of "un "
        assert_eq!(
            acts_of("3s", text, 0),
            vec![
                Act::SetClipboard("un ".into()),
                Act::Splice {
                    span: 0..3,
                    text: String::new(),
                    caret: 0
                },
            ],
        );
        // 5s on the last cluster: the count runs past the line's end, the
        // span clamps at line.end rather than reaching into the newline
        assert_eq!(
            acts_of("5s", text, 6),
            vec![
                Act::SetClipboard("é".into()),
                Act::Splice {
                    span: 6..8,
                    text: String::new(),
                    caret: 6
                },
            ],
        );
    }

    #[test]
    fn s_at_a_line_end_still_opens_insert() {
        let text = "un café\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "s", &view(text, &parsed, 8));
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![Act::Splice {
                span: 8..8,
                text: String::new(),
                caret: 8
            }]),
            "nothing to cut, but the session still opens",
        );
        assert_eq!(vim.mode, Mode::Insert);
    }

    #[test]
    fn capital_s_changes_the_line_like_cc() {
        let text = "une\ndeux\ntrois\n";
        assert_eq!(acts_of("S", text, 5), acts_of("cc", text, 5));
        // acts_of strips the checkpoints on both sides, so S's own is
        // asserted here rather than assumed
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "S", &view(text, &parsed, 5));
        let Outcome::Acts(acts) = outcome else {
            panic!("expected acts, got {outcome:?}");
        };
        assert_eq!(
            acts.first(),
            Some(&Act::Checkpoint),
            "S begins a change intent"
        );
    }

    #[test]
    fn the_dot_replays_s_with_its_typed_text() {
        let text = "un mot bleu\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        // s at 0 cuts "u" and opens a session at 0
        feed(&mut vim, "s", &view(text, &parsed, 0));
        assert_eq!(vim.mode, Mode::Insert);
        // the session typed "on"; escape captures text[0..2]
        let grown = "on mot bleu\n";
        let grown_parsed = blocks::segment(grown);
        vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(grown, &grown_parsed, 2),
        );
        // the dot on "bleu"'s first cluster changes it wholesale
        let outcome =
            stripped(feed(&mut vim, ".", &view(grown, &grown_parsed, 7)));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("b".into()),
                Act::Splice {
                    span: 7..8,
                    text: String::new(),
                    caret: 7
                },
                Act::Type("on".into()),
                Act::Place(8),
            ]),
        );
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn the_dot_replays_capital_s_with_its_typed_text() {
        let text = "une\ndeux\ntrois\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        // S on "deux" cuts the whole line and opens a session at its start
        feed(&mut vim, "S", &view(text, &parsed, 4));
        assert_eq!(vim.mode, Mode::Insert);
        // the session typed "quatre"; escape captures the grown span
        let grown = "une\nquatre\ntrois\n";
        let grown_parsed = blocks::segment(grown);
        vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(grown, &grown_parsed, 10),
        );
        // the dot on "trois" changes it wholesale, keeping its newline
        let outcome =
            stripped(feed(&mut vim, ".", &view(grown, &grown_parsed, 11)));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("trois\n".into()),
                Act::Splice {
                    span: 11..16,
                    text: String::new(),
                    caret: 11
                },
                Act::Type("quatre".into()),
                Act::Place(16),
            ]),
        );
        assert_eq!(vim.mode, Mode::Normal);
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

    // -- R replace mode
    // (adr/2026-08-replace-mode-session-and-backspace.md) --------------

    #[test]
    fn r_enters_replace_mode_with_one_checkpoint() {
        let text = "un été\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "R", &view(text, &parsed, 0));
        assert_eq!(outcome, Outcome::Acts(vec![Act::Checkpoint]));
        assert_eq!(vim.mode, Mode::Replace);
    }

    #[test]
    fn recasing_lands_in_the_recased_texts_own_bytes() {
        // Ⱥ is two bytes and ⱥ three, so every offset after it moves
        let text = "Ⱥa\n";
        let parsed = blocks::segment(text);
        let splice_of = |outcome: Outcome| match outcome {
            Outcome::Acts(acts) => acts
                .into_iter()
                .find_map(|act| match act {
                    Act::Splice { text, caret, .. } => Some((text, caret)),
                    _ => None,
                })
                .expect("a splice"),
            other => panic!("no acts: {other:?}"),
        };
        // a linewise gu leaves the caret on its character, the "a"
        let mut vim = normal();
        let (recased, caret) =
            splice_of(feed(&mut vim, "Vu", &view(text, &parsed, 2)));
        assert!(recased.starts_with("ⱥa"), "{recased:?}");
        assert_eq!(caret, 3);
        // ~ steps past the flipped run
        let mut vim = normal();
        let (flipped, caret) =
            splice_of(feed(&mut vim, "~", &view(text, &parsed, 0)));
        assert_eq!((flipped.as_str(), caret), ("ⱥ", 3));
        // and stays on the line's last cluster when the run reached its
        // end, in the flipped text's own bytes
        let text = "ȺȺ\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let (flipped, caret) =
            splice_of(feed(&mut vim, "2~", &view(text, &parsed, 0)));
        assert_eq!((flipped.as_str(), caret), ("ⱥⱥ", 3));
    }

    #[test]
    fn replace_past_the_trailing_newline_inserts_instead_of_reversing() {
        // the caret an opened note starts with: after the final newline,
        // where the line around it ends before it
        let text = "*\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "R", &view(text, &parsed, 2));
        let outcome = vim.handle(
            &character("h"),
            Modifiers::empty(),
            &view(text, &parsed, 2),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Splice {
                span: 2..2,
                text: "h".into(),
                caret: 3,
            }]),
        );
        assert_eq!(vim.replaced, vec![String::new()]);
    }

    #[test]
    fn replace_key_overwrites_each_cluster_and_moves_on() {
        let text0 = "un été\n";
        let parsed0 = blocks::segment(text0);
        let mut vim = normal();
        feed(&mut vim, "R", &view(text0, &parsed0, 0));

        let outcome = vim.handle(
            &character("X"),
            Modifiers::empty(),
            &view(text0, &parsed0, 0),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Splice {
                span: 0..1,
                text: "X".into(),
                caret: 1,
            }]),
            "the first cluster overwrites and the caret steps on",
        );

        // "u" became "X"; the session now sits before the "n"
        let text1 = "Xn été\n";
        let parsed1 = blocks::segment(text1);
        let outcome = vim.handle(
            &character("Y"),
            Modifiers::empty(),
            &view(text1, &parsed1, 1),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Splice {
                span: 1..2,
                text: "Y".into(),
                caret: 2,
            }]),
            "the second overwrites the next cluster",
        );
    }

    #[test]
    fn replace_key_appends_past_the_lines_end() {
        let text = "un\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "R", &view(text, &parsed, 2));
        let outcome = vim.handle(
            &character("!"),
            Modifiers::empty(),
            &view(text, &parsed, 2),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Splice {
                span: 2..2,
                text: "!".into(),
                caret: 3,
            }]),
            "nothing sits at the line's end, so the keystroke appends",
        );
    }

    #[test]
    fn replace_backspace_restores_the_previous_original() {
        // X replaced "u", Y replaced "n" — the session remembers both
        let text = "XY été\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(0),
            replaced: vec!["u".to_string(), "n".to_string()],
            ..Vim::default()
        };
        let outcome = vim.handle(
            &Key::Backspace,
            Modifiers::empty(),
            &view(text, &parsed, 2),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Splice {
                span: 1..2,
                text: "n".into(),
                caret: 1,
            }]),
            "the second original comes back",
        );
        assert_eq!(vim.replaced, vec!["u".to_string()], "one original popped");

        // "!" was appended past the line's end — its original is empty, so
        // restoring it simply deletes what was typed
        let text = "un!\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(2),
            replaced: vec![String::new()],
            ..Vim::default()
        };
        let outcome = vim.handle(
            &Key::Backspace,
            Modifiers::empty(),
            &view(text, &parsed, 3),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Splice {
                span: 2..3,
                text: String::new(),
                caret: 2,
            }]),
            "an empty original simply deletes what was typed",
        );
    }

    #[test]
    fn replace_backspace_restores_a_cluster_displaced_before_the_session_start()
     {
        // R, one overwrite, Backspace to restore it, Backspace again past
        // the session's start, then an overwrite there: the next Backspace
        // owes that new cluster its original back — the stack, not the
        // start offset, says what the session displaced
        // (adr/2026-08-replace-mode-session-and-backspace.md)
        let text = "abc\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "R", &view(text, &parsed, 1));
        vim.handle(
            &character("X"),
            Modifiers::empty(),
            &view(text, &parsed, 1),
        );
        assert_eq!(vim.replaced, vec!["b".to_string()]);

        // the splice landed: "abc" is "aXc", the caret past the X
        let typed = "aXc\n";
        let typed_parsed = blocks::segment(typed);
        assert_eq!(
            vim.handle(
                &Key::Backspace,
                Modifiers::empty(),
                &view(typed, &typed_parsed, 2),
            ),
            Outcome::Acts(vec![Act::Splice {
                span: 1..2,
                text: "b".into(),
                caret: 1,
            }]),
            "the original comes back",
        );
        assert!(vim.replaced.is_empty(), "and the stack is empty again");

        // nothing left to restore: the caret only moves, past the start
        assert_eq!(
            vim.handle(
                &Key::Backspace,
                Modifiers::empty(),
                &view(text, &parsed, 1),
            ),
            Outcome::Acts(vec![Act::Place(0)]),
        );
        // a fresh overwrite there displaces the a
        vim.handle(
            &character("Z"),
            Modifiers::empty(),
            &view(text, &parsed, 0),
        );
        assert_eq!(vim.replaced, vec!["a".to_string()]);
        let again = "Zbc\n";
        let again_parsed = blocks::segment(again);
        assert_eq!(
            vim.handle(
                &Key::Backspace,
                Modifiers::empty(),
                &view(again, &again_parsed, 1),
            ),
            Outcome::Acts(vec![Act::Splice {
                span: 0..1,
                text: "a".into(),
                caret: 0,
            }]),
            "the newly displaced cluster restores too",
        );
        assert!(
            vim.replaced.is_empty(),
            "and leaves no stale entry behind for Escape to publish"
        );
    }

    #[test]
    fn replace_backspace_past_the_session_start_only_moves_and_clamps_to_the_line_start()
     {
        let text = "un\ndeux\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(5),
            replaced: vec![],
            ..Vim::default()
        };
        let outcome = vim.handle(
            &Key::Backspace,
            Modifiers::empty(),
            &view(text, &parsed, 5),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Place(4)]),
            "it only moves"
        );

        let outcome = vim.handle(
            &Key::Backspace,
            Modifiers::empty(),
            &view(text, &parsed, 4),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Place(3)]),
            "onto the line's start"
        );

        let outcome = vim.handle(
            &Key::Backspace,
            Modifiers::empty(),
            &view(text, &parsed, 3),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(vec![Act::Place(3)]),
            "and clamps there rather than crossing into the line above"
        );
    }

    #[test]
    fn replace_escape_returns_to_normal_with_one_clipboard_write() {
        let text = "XY été\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(0),
            replaced: vec!["u".to_string(), "n".to_string()],
            last_change: Some(Change::Overwrite {
                typed: String::new(),
            }),
            ..Vim::default()
        };
        let outcome = vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(text, &parsed, 2),
        );
        assert_eq!(
            outcome,
            Outcome::Acts(
                vec![Act::SetClipboard("un".into()), Act::Place(1),]
            ),
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(
            vim.last_change,
            Some(Change::Overwrite { typed: "XY".into() }),
            "escape captures the session's typed text for the dot",
        );
    }

    #[test]
    fn replace_escape_with_nothing_typed_emits_no_clipboard_write() {
        let text = "un été\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(0),
            replaced: vec![],
            last_change: Some(Change::Overwrite {
                typed: String::new(),
            }),
            ..Vim::default()
        };
        let outcome = vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(text, &parsed, 0),
        );
        assert_eq!(outcome, Outcome::Acts(vec![Act::Place(0)]));
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn unbound_keys_in_replace_mode_swallow() {
        let text = "un été\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(0),
            ..Vim::default()
        };
        for key in [Key::Enter, Key::Tab, Key::ArrowLeft, Key::Delete] {
            assert_eq!(
                vim.handle(&key, Modifiers::empty(), &view(text, &parsed, 0)),
                Outcome::Swallow,
                "{key:?}"
            );
            assert_eq!(
                vim.mode,
                Mode::Replace,
                "the session holds under an unbound key"
            );
        }
    }

    #[test]
    fn the_dot_replays_the_replace_session_as_one_splice() {
        let text0 = "abc def\n";
        let parsed0 = blocks::segment(text0);
        let mut vim = normal();
        feed(&mut vim, "R", &view(text0, &parsed0, 0));
        vim.handle(
            &character("X"),
            Modifiers::empty(),
            &view(text0, &parsed0, 0),
        );
        let text1 = "Xbc def\n";
        let parsed1 = blocks::segment(text1);
        vim.handle(
            &character("Y"),
            Modifiers::empty(),
            &view(text1, &parsed1, 1),
        );
        let text2 = "XYc def\n";
        let parsed2 = blocks::segment(text2);
        vim.handle(
            &character("Z"),
            Modifiers::empty(),
            &view(text2, &parsed2, 2),
        );
        let text3 = "XYZ def\n";
        let parsed3 = blocks::segment(text3);
        vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(text3, &parsed3, 3),
        );
        assert_eq!(vim.mode, Mode::Normal);

        // the dot on "def" replays the whole session as one splice
        let outcome = feed(&mut vim, ".", &view(text3, &parsed3, 4));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::Checkpoint,
                Act::SetClipboard("def".into()),
                Act::Splice {
                    span: 4..7,
                    text: "XYZ".into(),
                    caret: 6,
                },
            ]),
        );
        assert_eq!(vim.mode, Mode::Normal, "the dot never opens a session");
    }

    #[test]
    fn the_dot_replays_an_overwrite_that_lands_on_nothing() {
        let text = "AB\nx\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Normal,
            last_change: Some(Change::Overwrite { typed: "AB".into() }),
            ..Vim::default()
        };
        // "x\n": the second line is one cluster long, and the recorded
        // session lands right on its end — nothing to overwrite there, so
        // no clipboard write joins the splice
        let outcome = feed(&mut vim, ".", &view(text, &parsed, 4));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 4..4,
                    text: "AB".into(),
                    caret: 5,
                },
            ]),
        );
    }

    #[test]
    fn the_dot_on_an_empty_replace_session_swallows() {
        let text = "un été\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Normal,
            last_change: Some(Change::Overwrite {
                typed: String::new(),
            }),
            ..Vim::default()
        };
        assert_eq!(
            feed(&mut vim, ".", &view(text, &parsed, 0)),
            Outcome::Swallow,
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
        // ip/ap now name a line, not a paragraph
        // (adr/2026-08-per-line-block-segmentation.md): "- deux cafés" is
        // its own block, content 23..36, range running through the
        // separator to 37
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        let outcome = feed(&mut vim, "dip", &view(NOTE, &parsed, 25));
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("- deux cafés\n".to_string()),
                Act::Splice {
                    span: 23..36,
                    text: String::new(),
                    caret: 23
                },
            ]),
            "ip is the block's content, linewise in the register"
        );
        let mut vim = normal();
        let outcome = feed(&mut vim, "dap", &view(NOTE, &parsed, 25));
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("- deux cafés\n".to_string()),
                Act::Splice {
                    span: 23..37,
                    text: String::new(),
                    caret: 23
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
    fn ds_drops_both_delimiters() {
        // ds( from inside a bracket pair: both delimiters cut to the
        // register, the inner text left in place
        let text = "un (mot) la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let outcome = feed(&mut vim, "ds(", &view(text, &parsed, 5));
        let Outcome::Acts(acts) = outcome.clone() else {
            panic!("expected acts, got {outcome:?}");
        };
        assert_eq!(
            acts.first(),
            Some(&Act::Checkpoint),
            "ds begins a change intent"
        );
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![
                Act::SetClipboard("()".into()),
                Act::Splice {
                    span: 3..8,
                    text: "mot".into(),
                    caret: 3
                },
            ]),
        );
        // ds* from inside a Typst emphasis pair: the same shape
        let text = "un *mot* la\n";
        assert_eq!(
            acts_of("ds*", text, 5),
            vec![
                Act::SetClipboard("**".into()),
                Act::Splice {
                    span: 3..8,
                    text: "mot".into(),
                    caret: 3
                },
            ],
        );
    }

    #[test]
    fn cs_replaces_both_delimiters() {
        // cs*( : a quote-like pair swaps for a bracket, bare on both sides
        let text = "un *mot* la\n";
        assert_eq!(
            acts_of("cs*(", text, 5),
            vec![
                Act::SetClipboard("**".into()),
                Act::Splice {
                    span: 3..8,
                    text: "(mot)".into(),
                    caret: 3
                },
            ],
        );
        // cs(' : a bracket swaps for a quote, bare on both sides — the
        // opening key's padding never reaches cs
        let text = "un (mot) la\n";
        assert_eq!(
            acts_of("cs('", text, 5),
            vec![
                Act::SetClipboard("()".into()),
                Act::Splice {
                    span: 3..8,
                    text: "'mot'".into(),
                    caret: 3
                },
            ],
        );
    }

    /// Every target key, against text that holds the pair it names: the
    /// delimiters cut and the span they covered, so a swapped
    /// `surround_kind` entry fails here rather than passing on
    /// target-free text (adr/2026-08-surround-pair-set-and-padding.md).
    #[test]
    fn every_surround_target_key_finds_its_own_pair() {
        for (key, text, span) in [
            ("*", "un *mot* la\n", 3..8),
            ("_", "un _mot_ la\n", 3..8),
            ("'", "un 'mot' la\n", 3..8),
            ("\"", "un \"mot\" la\n", 3..8),
            ("`", "un `mot` la\n", 3..8),
            ("(", "un (mot) la\n", 3..8),
            (")", "un (mot) la\n", 3..8),
            ("b", "un (mot) la\n", 3..8),
            ("[", "un [mot] la\n", 3..8),
            ("]", "un [mot] la\n", 3..8),
            ("{", "un {mot} la\n", 3..8),
            ("}", "un {mot} la\n", 3..8),
            ("B", "un {mot} la\n", 3..8),
            ("<", "un <mot> la\n", 3..8),
            (">", "un <mot> la\n", 3..8),
            // the guillemets are two bytes apiece, so their pair spans wider
            ("«", "un «mot» la\n", 3..10),
            ("»", "un «mot» la\n", 3..10),
        ] {
            let keys = format!("ds{key}");
            let open =
                text.get(span.start..).and_then(|rest| rest.chars().next());
            let close = text
                .get(..span.end)
                .and_then(|before| before.chars().next_back());
            let cut =
                format!("{}{}", open.unwrap_or(' '), close.unwrap_or(' '));
            assert_eq!(
                acts_of(&keys, text, 5),
                vec![
                    Act::SetClipboard(cut),
                    Act::Splice {
                        span: span.clone(),
                        text: "mot".into(),
                        caret: span.start,
                    },
                ],
                "{keys}"
            );
        }
    }

    /// Every pair key, as the delimiters a wrap writes: the splice text
    /// carries both delimiters *and* the padding, so any swapped or
    /// re-flagged `surround_pair` entry fails here — only the opening
    /// bracket keys, guillemet included, pad
    /// (adr/2026-08-surround-pair-set-and-padding.md).
    #[test]
    fn every_wrap_key_writes_its_own_delimiters_and_padding() {
        let text = "un mot la\n";
        for (key, written) in [
            ("*", "*mot*"),
            ("_", "_mot_"),
            ("'", "'mot'"),
            ("\"", "\"mot\""),
            ("`", "`mot`"),
            ("(", "( mot )"),
            (")", "(mot)"),
            ("b", "(mot)"),
            ("[", "[ mot ]"),
            ("]", "[mot]"),
            ("{", "{ mot }"),
            ("}", "{mot}"),
            ("B", "{mot}"),
            ("<", "< mot >"),
            (">", "<mot>"),
            ("«", "« mot »"),
            ("»", "«mot»"),
        ] {
            let keys = format!("ysiw{key}");
            assert_eq!(
                acts_of(&keys, text, 4),
                vec![Act::Splice {
                    span: 3..6,
                    text: written.into(),
                    caret: 3,
                }],
                "{keys}"
            );
        }
    }

    #[test]
    fn the_guillemets_answer_the_surround_keys_a_french_vault_types() {
        // di« already named them; ds, cs and ys answer them too
        // (adr/2026-08-surround-pair-set-and-padding.md)
        let text = "un «mot» la\n";
        assert_eq!(
            acts_of("cs«\"", text, 6),
            vec![
                Act::SetClipboard("«»".into()),
                Act::Splice {
                    span: 3..10,
                    text: "\"mot\"".into(),
                    caret: 3
                },
            ],
        );
        // ysiw« : the opening guillemet pads, as French typography spaces it
        let text = "un mot la\n";
        assert_eq!(
            acts_of("ysiw«", text, 4),
            vec![Act::Splice {
                span: 3..6,
                text: "« mot »".into(),
                caret: 3
            }],
        );
        // ysiw» : the closing key is bare, as every closing key is
        assert_eq!(
            acts_of("ysiw»", text, 4),
            vec![Act::Splice {
                span: 3..6,
                text: "«mot»".into(),
                caret: 3
            }],
        );
    }

    #[test]
    fn a_surround_with_no_target_aborts_quietly() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        // dsb: a valid key, but no parentheses stand on the heading line
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "dsb", &sight), Outcome::Swallow);
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: false
            }]),
            "the grammar recovered"
        );
    }

    #[test]
    fn an_unknown_surround_key_aborts_quietly() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        // ds then a key outside the surround pair set
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "dsz", &sight), Outcome::Swallow);
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: false
            }]),
            "the grammar recovered"
        );
        // cs<old> with a valid old standing on a real target, but a new key
        // outside the set: the swallow comes from the key, not from a
        // missing pair, so the None arm of Prefix::SurroundNew is what this
        // pins
        let text = "un (mot) la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "cs(", &view(text, &parsed, 5)),
            Outcome::Swallow,
            "the pair is there — cs is only waiting for its new key"
        );
        assert_eq!(
            vim.handle(
                &character("z"),
                Modifiers::empty(),
                &view(text, &parsed, 5)
            ),
            Outcome::Swallow,
        );
        assert_eq!(
            feed(&mut vim, "l", &view(text, &parsed, 5)),
            Outcome::Acts(vec![Act::Place(6)]),
            "the grammar recovered"
        );
    }

    #[test]
    fn the_dot_replays_ds_and_cs_at_a_second_occurrence() {
        let text = "un (a) et (b)\n";
        let parsed = blocks::segment(text);
        // ds( on the first pair, then . on the second
        let mut vim = normal();
        feed(&mut vim, "ds(", &view(text, &parsed, 4));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 11))),
            Outcome::Acts(vec![
                Act::SetClipboard("()".into()),
                Act::Splice {
                    span: 10..13,
                    text: "b".into(),
                    caret: 10
                },
            ]),
        );
        // cs(' on the first pair, then . on the second
        let mut vim = normal();
        feed(&mut vim, "cs('", &view(text, &parsed, 4));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 11))),
            Outcome::Acts(vec![
                Act::SetClipboard("()".into()),
                Act::Splice {
                    span: 10..13,
                    text: "'b'".into(),
                    caret: 10
                },
            ]),
        );
    }

    // -- ys and visual S: the wrapping half of surround ----------------------

    #[test]
    fn ys_object_wraps_padded_or_bare() {
        let text = "un mot la\n";
        let parsed = blocks::segment(text);
        // ysiw( : an opening bracket pads
        let mut vim = normal();
        let outcome = feed(&mut vim, "ysiw(", &view(text, &parsed, 4));
        let Outcome::Acts(acts) = outcome.clone() else {
            panic!("expected acts, got {outcome:?}");
        };
        assert_eq!(
            acts.first(),
            Some(&Act::Checkpoint),
            "a wrap begins a change intent"
        );
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![Act::Splice {
                span: 3..6,
                text: "( mot )".into(),
                caret: 3
            }]),
        );
        // ysiw) : the closing key is bare
        assert_eq!(
            acts_of("ysiw)", text, 4),
            vec![Act::Splice {
                span: 3..6,
                text: "(mot)".into(),
                caret: 3
            }],
        );
    }

    #[test]
    fn ys_motion_wraps_with_a_quote() {
        let text = "un mot la\n";
        // ysw : the motion's own span (dw's), wrapped bare
        assert_eq!(
            acts_of("ysw\"", text, 0),
            vec![Act::Splice {
                span: 0..3,
                text: "\"un \"".into(),
                caret: 0
            }],
        );
        // and the dot re-resolves that motion at the caret it finds
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "ysw\"", &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 3))),
            Outcome::Acts(vec![Act::Splice {
                span: 3..7,
                text: "\"mot \"".into(),
                caret: 3
            }]),
        );
    }

    #[test]
    fn ys_wraps_with_a_quote_like_key() {
        let text = "un mot la\n";
        // ysiw* : Typst emphasis, bare on both sides like the quotes
        assert_eq!(
            acts_of("ysiw*", text, 4),
            vec![Act::Splice {
                span: 3..6,
                text: "*mot*".into(),
                caret: 3
            }],
        );
    }

    #[test]
    fn visual_s_wraps_charwise_and_linewise() {
        // char-wise: both end clusters land inside the wrap
        let text = "un mot la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "v", &view(text, &parsed, 3));
        assert_eq!(
            vim.handle(
                &character("S"),
                Modifiers::empty(),
                &spread(text, &parsed, 3, 5),
            ),
            Outcome::Swallow,
            "S waits for the pair key"
        );
        assert_eq!(
            vim.mode,
            Mode::Visual(VisualKind::Char),
            "the selection keeps its owner until the pair key lands"
        );
        let outcome = vim.handle(
            &character("("),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 5),
        );
        let Outcome::Acts(acts) = outcome.clone() else {
            panic!("expected acts, got {outcome:?}");
        };
        assert_eq!(
            acts.first(),
            Some(&Act::Checkpoint),
            "the wrap begins a change intent"
        );
        assert_eq!(
            stripped(outcome),
            Outcome::Acts(vec![Act::Splice {
                span: 3..6,
                text: "( mot )".into(),
                caret: 3
            }]),
        );
        assert_eq!(vim.mode, Mode::Normal, "and the wrap ends the selection");

        // line-wise: the whole lines, bare
        let text = "une\ndeux\ntrois\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "V", &view(text, &parsed, 1));
        vim.handle(
            &character("S"),
            Modifiers::empty(),
            &spread(text, &parsed, 1, 5),
        );
        assert_eq!(
            vim.mode,
            Mode::Visual(VisualKind::Line),
            "the whole-line highlight stays painted for that keystroke"
        );
        assert_eq!(
            stripped(vim.handle(
                &character("*"),
                Modifiers::empty(),
                &spread(text, &parsed, 1, 5),
            )),
            Outcome::Acts(vec![Act::Splice {
                span: 0..9,
                text: "*une\ndeux\n*".into(),
                caret: 0
            }]),
        );
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn an_unknown_pair_key_after_visual_s_aborts_to_normal() {
        // the selection must not stay painted with no owner once the
        // pending wrap dies (adr/2026-08-surround-pair-set-and-padding.md)
        let text = "un mot la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "v", &view(text, &parsed, 3));
        vim.handle(
            &character("S"),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 5),
        );
        assert_eq!(
            vim.handle(
                &character("z"),
                Modifiers::empty(),
                &spread(text, &parsed, 3, 5),
            ),
            Outcome::Swallow,
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(
            feed(&mut vim, "l", &view(text, &parsed, 3)),
            Outcome::Acts(vec![Act::Place(4)]),
            "the grammar recovered, in normal mode"
        );
    }

    #[test]
    fn escape_after_visual_s_aborts_to_normal_in_one_key() {
        // the pending wrap dies the same way whichever key kills it: one
        // Escape, not two (adr/2026-08-surround-pair-set-and-padding.md)
        let text = "un mot la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "v", &view(text, &parsed, 3));
        vim.handle(
            &character("S"),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 5),
        );
        assert_eq!(
            vim.handle(
                &Key::Escape,
                Modifiers::empty(),
                &spread(text, &parsed, 3, 5),
            ),
            Outcome::Swallow,
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(
            feed(&mut vim, "l", &view(text, &parsed, 3)),
            Outcome::Acts(vec![Act::Place(4)]),
            "the grammar recovered, in normal mode"
        );
    }

    #[test]
    fn wrapping_never_touches_the_clipboard() {
        let text = "un mot la\n";
        let parsed = blocks::segment(text);
        for keys in ["ysiw(", "ysiw)", "ysw\"", "ysiw*", "yss(", "yss*"] {
            let mut vim = normal();
            let outcome = feed(&mut vim, keys, &view(text, &parsed, 4));
            let Outcome::Acts(acts) = outcome else {
                panic!("{keys}: expected acts, got {outcome:?}");
            };
            assert!(
                !acts.iter().any(|act| matches!(act, Act::SetClipboard(_))),
                "{keys} touched the clipboard"
            );
        }
        // visual S wraps through the same splice, and cuts nothing either
        let mut vim = normal();
        feed(&mut vim, "v", &view(text, &parsed, 3));
        vim.handle(
            &character("S"),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 5),
        );
        let outcome = vim.handle(
            &character("("),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 5),
        );
        let Outcome::Acts(acts) = outcome else {
            panic!("visual S: expected acts, got {outcome:?}");
        };
        assert!(
            !acts.iter().any(|act| matches!(act, Act::SetClipboard(_))),
            "visual S touched the clipboard"
        );
    }

    #[test]
    fn yss_wraps_the_current_line_and_ysy_is_nothing() {
        let text = "une\ndeux\n";
        // yss* : the line's text, wrapped bare — the ending stays outside
        // the pair, or the closer would head the next line
        assert_eq!(
            acts_of("yss*", text, 1),
            vec![Act::Splice {
                span: 0..3,
                text: "*une*".into(),
                caret: 0
            }],
        );
        // yss( : the opening bracket pads, exactly as ysiw( does
        assert_eq!(
            acts_of("yss(", text, 1),
            vec![Act::Splice {
                span: 0..3,
                text: "( une )".into(),
                caret: 0
            }],
        );
        // ysy is not the idiom: the stray y aborts the whole thing, and no
        // pair key is pending afterward
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "ysy", &view(text, &parsed, 1)),
            Outcome::Swallow,
        );
        assert_eq!(
            vim.handle(
                &character("("),
                Modifiers::empty(),
                &view(text, &parsed, 1)
            ),
            Outcome::Swallow,
            "( is just an unbound key in normal mode"
        );
        assert_eq!(vim.last_change, None, "and nothing was recorded");
    }

    #[test]
    fn the_dot_replays_yss_at_a_new_line() {
        let text = "une\ndeux\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "yss*", &view(text, &parsed, 1));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 5))),
            Outcome::Acts(vec![Act::Splice {
                span: 4..8,
                text: "*deux*".into(),
                caret: 4
            }]),
        );
    }

    #[test]
    fn a_visual_wrap_leaves_a_pending_ys_record_alone() {
        // ysiw( records the pair it wrote; a visual S over the same text
        // must not rewrite that record's pair, or the next dot replays the
        // wrong padding (adr/2026-08-surround-pair-set-and-padding.md)
        let text = "un mot la fin\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "ysiw(", &view(text, &parsed, 4));
        // the splice landed: "un ( mot ) la fin"
        let wrapped = "un ( mot ) la fin\n";
        let wrapped_parsed = blocks::segment(wrapped);
        feed(&mut vim, "v", &view(wrapped, &wrapped_parsed, 11));
        vim.handle(
            &character("S"),
            Modifiers::empty(),
            &spread(wrapped, &wrapped_parsed, 11, 12),
        );
        vim.handle(
            &character(")"),
            Modifiers::empty(),
            &spread(wrapped, &wrapped_parsed, 11, 12),
        );
        // the dot on "fin" still replays ys's own padded pair
        let visual = "un ( mot ) (la) fin\n";
        let visual_parsed = blocks::segment(visual);
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(visual, &visual_parsed, 16))),
            Outcome::Acts(vec![Act::Splice {
                span: 16..19,
                text: "( fin )".into(),
                caret: 16
            }]),
        );
    }

    #[test]
    fn an_unknown_wrap_pair_aborts_quietly() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 13);
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "ysiwz", &sight), Outcome::Swallow);
        assert_eq!(
            feed(&mut vim, "l", &sight),
            Outcome::Acts(vec![Act::Place(14)]),
            "the grammar recovered"
        );
    }

    #[test]
    fn a_wrap_noun_that_finds_nothing_aborts_before_the_pair() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, NOTE.len());
        let mut vim = normal();
        assert_eq!(feed(&mut vim, "ysiw", &sight), Outcome::Swallow);
        // had a pair still been pending, "(" would have spliced a wrap;
        // instead it is just an unbound key in normal mode
        assert_eq!(
            vim.handle(&character("("), Modifiers::empty(), &sight),
            Outcome::Swallow,
        );
    }

    #[test]
    fn the_dot_replays_ys_at_a_new_caret_with_the_same_pair_and_padding() {
        let text = "mot un mot deux\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "ysiw(", &view(text, &parsed, 1));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 8))),
            Outcome::Acts(vec![Act::Splice {
                span: 7..10,
                text: "( mot )".into(),
                caret: 7
            }]),
        );
    }

    #[test]
    fn the_dot_after_visual_s_replays_the_previous_change() {
        let text = "un mot la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        // a plain change first, so last_change is something concrete
        feed(&mut vim, "x", &view(text, &parsed, 0));
        assert_eq!(
            vim.last_change,
            Some(Change::Cut {
                forward: true,
                count: 1
            }),
        );
        // visual S wraps but records nothing
        feed(&mut vim, "v", &view(text, &parsed, 3));
        vim.handle(
            &character("S"),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 5),
        );
        vim.handle(
            &character("("),
            Modifiers::empty(),
            &spread(text, &parsed, 3, 5),
        );
        assert_eq!(
            vim.last_change,
            Some(Change::Cut {
                forward: true,
                count: 1
            }),
            "visual S left the previous change standing"
        );
        // the dot still replays the x, not the wrap
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 7))),
            Outcome::Acts(vec![
                Act::SetClipboard("l".into()),
                Act::Splice {
                    span: 7..8,
                    text: String::new(),
                    caret: 7
                },
            ]),
        );
    }

    #[test]
    fn a_dot_with_no_recorded_pair_swallows() {
        let text = "un mot la\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        // ysiw arms the wrap and finds its noun, then Escape aborts
        // before the pair key ever lands — last_change keeps the
        // pair-less record exactly as the escape ladder leaves any other
        // pending grammar (adr/2026-08-escape-ladder-editor-wide-mode.md)
        feed(&mut vim, "ysiw", &view(text, &parsed, 4));
        vim.handle(&Key::Escape, Modifiers::empty(), &view(text, &parsed, 4));
        assert_eq!(
            vim.last_change,
            Some(Change::Wrap {
                noun: WrapNoun::Object {
                    kind: ObjectKind::Word,
                    around: false
                },
                count: 1,
                pair: None
            }),
        );
        assert_eq!(
            feed(&mut vim, ".", &view(text, &parsed, 4)),
            Outcome::Swallow,
        );
    }

    #[test]
    fn the_dot_on_ys_aborts_when_the_noun_finds_nothing() {
        let parsed = blocks::segment(NOTE);
        let mut vim = normal();
        feed(&mut vim, "ysiw(", &view(NOTE, &parsed, 13));
        assert_eq!(
            feed(&mut vim, ".", &view(NOTE, &parsed, NOTE.len())),
            Outcome::Swallow,
        );
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
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: false
            }]),
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
        // escape kills a pending verb, and shift+escape kills it on the
        // way out of the note
        let mut vim = normal();
        feed(&mut vim, "d", &sight);
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::empty(), &sight),
            Outcome::Swallow
        );
        let mut vim = normal();
        feed(&mut vim, "d", &sight);
        assert_eq!(
            vim.handle(&Key::Escape, Modifiers::SHIFT, &sight),
            Outcome::Acts(vec![Act::Deactivate]),
        );
        assert_eq!(
            feed(&mut vim, "j", &sight),
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: false
            }]),
            "the pending verb died with the exit"
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
            stripped(outcome),
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
            stripped(outcome),
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
            stripped(outcome),
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
            stripped(outcome),
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
            stripped(feed(&mut vim, "~", &view(text, &parsed, 0))),
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
        // counts still compose, for the executor to resolve
        assert_eq!(
            feed(&mut vim, "2j", &spread(NOTE, &parsed, 2, 4)),
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 2,
                extend: true
            }]),
        );
        // o swaps the ends, unbound keys stay inert
        assert_eq!(
            feed(&mut vim, "o", &spread(NOTE, &parsed, 2, 27)),
            Outcome::Acts(vec![Act::SwapEnds]),
        );
        assert_eq!(
            feed(&mut vim, "q", &spread(NOTE, &parsed, 27, 2)),
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
            stripped(outcome),
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
            stripped(outcome),
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
            stripped(outcome),
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
            stripped(outcome),
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
            vim.handle(
                &Key::Enter,
                Modifiers::empty(),
                &view(text, &parsed, 5)
            ),
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
        // plain j/k hand the direction to the executor, the anchor held
        assert_eq!(
            feed(&mut vim, "k", &spread(text, &parsed, 10, 25)),
            Outcome::Acts(vec![Act::WalkVisual {
                down: false,
                count: 1,
                extend: true
            }]),
        );
        assert_eq!(
            feed(&mut vim, "j", &spread(text, &parsed, 10, 25)),
            Outcome::Acts(vec![Act::WalkVisual {
                down: true,
                count: 1,
                extend: true
            }]),
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
            stripped(feed(&mut vim, "p", &sight)),
            Outcome::Acts(vec![Act::Paste {
                before: false,
                count: 1
            }]),
        );
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "3P", &sight)),
            Outcome::Acts(vec![Act::Paste {
                before: true,
                count: 3
            }]),
        );
    }

    // -- phase 5: undo keys, the dot, search ---------------------------------

    #[test]
    fn every_change_class_checkpoints_and_nothing_else_does() {
        let text = "un mot\n";
        let parsed = blocks::segment(text);
        let first_act = |keys: &str| -> Option<Act> {
            let mut vim = normal();
            match feed(&mut vim, keys, &view(text, &parsed, 0)) {
                Outcome::Acts(acts) => acts.into_iter().next(),
                _ => None,
            }
        };
        for keys in [
            "dw", "cw", "dd", "x", "rz", "~", "p", "i", "o", "ysiw(", "yss(",
        ] {
            assert_eq!(
                first_act(keys),
                Some(Act::Checkpoint),
                "{keys} begins a change intent"
            );
        }
        for keys in ["yy", "yw", "w", "$"] {
            assert_ne!(
                first_act(keys),
                Some(Act::Checkpoint),
                "{keys} changes nothing"
            );
        }
    }

    #[test]
    fn u_and_ctrl_r_reach_the_history() {
        let parsed = blocks::segment(NOTE);
        let sight = view(NOTE, &parsed, 0);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "u", &sight),
            Outcome::Acts(vec![Act::Undo]),
        );
        assert_eq!(
            vim.handle(&character("r"), Modifiers::CONTROL, &sight),
            Outcome::Acts(vec![Act::Redo]),
            "the one ctrl carve-out"
        );
        // in insert, Ctrl+R passes through like any chord
        let mut vim = insert();
        assert_eq!(
            vim.handle(&character("r"), Modifiers::CONTROL, &sight),
            Outcome::Pass,
        );
    }

    #[test]
    fn the_dot_replays_each_change_class() {
        let text = "un mot bleu\n";
        let parsed = blocks::segment(text);
        // dd then . — same spans re-resolved at the caret it finds
        let mut vim = normal();
        feed(&mut vim, "dd", &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 0))),
            Outcome::Acts(vec![
                Act::SetClipboard("un mot bleu\n".into()),
                Act::Splice {
                    span: 0..12,
                    text: String::new(),
                    caret: 0
                },
            ]),
        );
        // x then 3. — the count ahead of the dot overrides
        let mut vim = normal();
        feed(&mut vim, "x", &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, "3.", &view(text, &parsed, 0))),
            Outcome::Acts(vec![
                Act::SetClipboard("un ".into()),
                Act::Splice {
                    span: 0..3,
                    text: String::new(),
                    caret: 0
                },
            ]),
        );
        // r and ~ and p replay too
        let mut vim = normal();
        feed(&mut vim, "rz", &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 3))),
            Outcome::Acts(vec![Act::Splice {
                span: 3..4,
                text: "z".into(),
                caret: 3
            }]),
        );
        let mut vim = normal();
        feed(&mut vim, "~", &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 3))),
            Outcome::Acts(vec![Act::Splice {
                span: 3..4,
                text: "M".into(),
                caret: 4
            }]),
        );
        let mut vim = normal();
        feed(&mut vim, "2p", &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Paste {
                before: false,
                count: 2
            }]),
        );
        // an object replays at the new caret
        let mut vim = normal();
        feed(&mut vim, "diw", &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 3))),
            Outcome::Acts(vec![
                Act::SetClipboard("mot".into()),
                Act::Splice {
                    span: 3..6,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        // with nothing recorded the dot is inert
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, ".", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
    }

    #[test]
    fn the_dot_replays_insert_sessions_without_reopening_them() {
        let text = "un mot\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        // a at 0 → typing begins at 1; the session typed "xy"
        feed(&mut vim, "a", &view(text, &parsed, 0));
        assert_eq!(vim.mode, Mode::Insert);
        // escape captures text[1..3] as the session's text
        let grown = "uxyn mot\n";
        let grown_parsed = blocks::segment(grown);
        vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(grown, &grown_parsed, 3),
        );
        assert_eq!(vim.mode, Mode::Normal);
        // the dot replays: land after the cluster, type, rest on the tail
        let outcome =
            stripped(feed(&mut vim, ".", &view(grown, &grown_parsed, 4)));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::Place(5),
                Act::Type("xy".into()),
                Act::Place(6),
            ]),
        );
        assert_eq!(vim.mode, Mode::Normal, "the dot never opens a session");
    }

    #[test]
    fn the_dot_replays_a_change_with_its_typed_text() {
        let text = "un mot bleu\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        // cw at 3 deletes "mot" and opens a session at 3
        feed(&mut vim, "cw", &view(text, &parsed, 3));
        assert_eq!(vim.mode, Mode::Insert);
        // the session typed "champ"; escape captures text[3..8]
        let grown = "un champ bleu\n";
        let grown_parsed = blocks::segment(grown);
        vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view(grown, &grown_parsed, 8),
        );
        // the dot at "bleu" changes it wholesale
        let outcome =
            stripped(feed(&mut vim, ".", &view(grown, &grown_parsed, 9)));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("bleu".into()),
                Act::Splice {
                    span: 9..13,
                    text: String::new(),
                    caret: 9
                },
                Act::Type("champ".into()),
                Act::Place(13),
            ]),
        );
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn a_visual_change_session_closes_without_a_recording() {
        // visual c sets a session going but records nothing: escape's
        // capture finds no slot and shrugs
        let text = "un mot\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        vim.handle(
            &character("c"),
            Modifiers::empty(),
            &spread(text, &parsed, 0, 1),
        );
        assert_eq!(vim.mode, Mode::Insert);
        let outcome = vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &view("mot\n", &blocks::segment("mot\n"), 0),
        );
        assert!(matches!(outcome, Outcome::Acts(_)));
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn a_dot_whose_noun_finds_nothing_stays_put() {
        let parsed = blocks::segment(NOTE);
        // diw recorded, then the dot on the empty last line: no word
        let mut vim = normal();
        feed(&mut vim, "diw", &view(NOTE, &parsed, 2));
        assert_eq!(
            feed(&mut vim, ".", &view(NOTE, &parsed, NOTE.len())),
            Outcome::Swallow,
        );
        // and the change flavour declines the same way
        let mut vim = normal();
        feed(&mut vim, "ciw", &view(NOTE, &parsed, 2));
        vim.handle(&Key::Escape, Modifiers::empty(), &view(NOTE, &parsed, 2));
        assert_eq!(
            feed(&mut vim, ".", &view(NOTE, &parsed, NOTE.len())),
            Outcome::Swallow,
        );
    }

    #[test]
    fn yank_objects_record_nothing_and_empty_sessions_replay_bare() {
        let text = "un mot bleu\n";
        let parsed = blocks::segment(text);
        // yiw is not a change: the dot after it finds nothing to repeat
        let mut vim = normal();
        assert!(matches!(
            feed(&mut vim, "yiw", &view(text, &parsed, 0)),
            Outcome::Acts(_)
        ));
        assert_eq!(
            feed(&mut vim, ".", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
        // an abandoned cw replays its cut alone, typing nothing
        let mut vim = normal();
        feed(&mut vim, "cw", &view(text, &parsed, 0));
        vim.handle(&Key::Escape, Modifiers::empty(), &view(text, &parsed, 0));
        let outcome = stripped(feed(&mut vim, ".", &view(text, &parsed, 3)));
        assert_eq!(
            outcome,
            Outcome::Acts(vec![
                Act::SetClipboard("mot".into()),
                Act::Splice {
                    span: 3..6,
                    text: String::new(),
                    caret: 3
                },
            ]),
        );
        // and an abandoned i replays as a bare landing
        let mut vim = normal();
        feed(&mut vim, "i", &view(text, &parsed, 0));
        vim.handle(&Key::Escape, Modifiers::empty(), &view(text, &parsed, 0));
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 3))),
            Outcome::Acts(vec![]),
        );
    }

    #[test]
    fn a_fresh_note_begins_thinking_but_keeps_its_memory() {
        let text = "un mot\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "x", &view(text, &parsed, 0));
        feed(&mut vim, "fo", &view(text, &parsed, 0));
        vim.commit_search("mot".to_string());
        feed(&mut vim, "d", &view(text, &parsed, 0));
        feed(&mut vim, "i", &view(text, &parsed, 0));

        vim.note_opened();
        assert_eq!(vim.mode, Mode::Normal, "thinking, not writing");
        // the pending verb died with the note; the memory survives
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(text, &parsed, 3))),
            Outcome::Acts(vec![
                Act::SetClipboard("m".into()),
                Act::Splice {
                    span: 3..4,
                    text: String::new(),
                    caret: 3
                },
            ]),
            "the dot still remembers the cut"
        );
        assert_eq!(
            feed(&mut vim, "n", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(3)]),
            "the pattern survived the switch"
        );
    }

    #[test]
    fn slash_opens_the_prompt_and_n_walks_the_pattern() {
        let text = "Un café.\n\nEncore un Café noir.\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "/", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::OpenSearch]),
        );
        // n before any commit is inert
        assert_eq!(
            feed(&mut vim, "n", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
        vim.commit_search("café".to_string());
        assert_eq!(
            feed(&mut vim, "n", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(3)]),
        );
        assert_eq!(
            feed(&mut vim, "n", &view(text, &parsed, 3)),
            Outcome::Acts(vec![Act::Place(21)]),
        );
        assert_eq!(
            feed(&mut vim, "N", &view(text, &parsed, 3)),
            Outcome::Acts(vec![Act::Place(21)]),
            "N wraps backward"
        );
        // a pattern the note lost fails quietly
        vim.commit_search("thé".to_string());
        assert_eq!(
            feed(&mut vim, "n", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
        // an empty commit changes nothing
        vim.commit_search(String::new());
        assert_eq!(
            feed(&mut vim, "n", &view(text, &parsed, 0)),
            Outcome::Swallow,
            "the earlier pattern was thé, still absent"
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
        // per-line blocks make almost every line's next row exactly one
        // newline away — the one row with nothing following it at all is
        // the note's own trailing empty line
        // (adr/2026-08-per-line-block-segmentation.md); p there opens with
        // the break the body carried, same as ever
        let parsed = blocks::segment(NOTE);
        let (span, body, caret) = motions::paste_spec(
            NOTE,
            &parsed,
            NOTE.len(),
            "ligne\n",
            false,
            1,
        );
        assert_eq!(
            (span, body.as_str(), caret),
            (NOTE.len()..NOTE.len(), "\nligne", NOTE.len() + 1)
        );
    }

    // -- Tab: one indent level, in every mode but R -------------------------
    // (adr/2026-08-tab-indents-in-every-mode.md)

    /// One Tab press: the note it leaves and where the caret lands, with
    /// the single checkpoint asserted on the way through.
    fn tabbed(vim: &mut Vim, view: &View, deeper: bool) -> (String, usize) {
        let modifiers = if deeper {
            Modifiers::empty()
        } else {
            Modifiers::SHIFT
        };
        match vim.handle(&Key::Tab, modifiers, view) {
            Outcome::Acts(acts) => match acts.as_slice() {
                [Act::Checkpoint, Act::Splice { span, text, caret }] => {
                    let mut note = view.text.to_string();
                    note.replace_range(span.clone(), text);
                    (note, *caret)
                }
                other => panic!("expected one checkpointed splice: {other:?}"),
            },
            other => panic!("expected acts, got {other:?}"),
        }
    }

    #[test]
    fn tab_moves_the_caret_line_a_level_in_whatever_column_it_sits_at() {
        let text = "- une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        let (note, caret) = tabbed(&mut vim, &view(text, &parsed, 5), true);
        assert_eq!(note, "  - une idée\n");
        assert_eq!(caret, 7, "the caret rode the two spaces");
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn shift_tab_moves_it_back_out() {
        let text = "  - une idée\n";
        let parsed = blocks::segment(text);
        let (note, caret) =
            tabbed(&mut normal(), &view(text, &parsed, 7), false);
        assert_eq!(note, "- une idée\n");
        assert_eq!(caret, 5);
    }

    #[test]
    fn shift_tab_takes_the_one_space_a_half_level_left() {
        let text = " - une idée\n";
        let parsed = blocks::segment(text);
        let (note, _) = tabbed(&mut normal(), &view(text, &parsed, 6), false);
        assert_eq!(note, "- une idée\n");
    }

    #[test]
    fn shift_tab_inside_the_whitespace_lands_the_caret_at_the_margin() {
        let text = "    - une idée\n";
        let parsed = blocks::segment(text);
        let (note, caret) =
            tabbed(&mut normal(), &view(text, &parsed, 1), false);
        assert_eq!(note, "  - une idée\n");
        assert_eq!(caret, 0, "clamped onto the line it could not precede");
    }

    #[test]
    fn a_press_with_nothing_to_move_swallows_without_checkpointing() {
        // a bare line dedenting, and a blank line either way
        for (text, head, deeper) in
            [("- une idée\n", 3, false), ("\n", 0, true)]
        {
            let parsed = blocks::segment(text);
            let modifiers = if deeper {
                Modifiers::empty()
            } else {
                Modifiers::SHIFT
            };
            assert_eq!(
                normal().handle(
                    &Key::Tab,
                    modifiers,
                    &view(text, &parsed, head)
                ),
                Outcome::Swallow,
                "{text:?}"
            );
        }
    }

    #[test]
    fn insert_mode_tabs_without_leaving_insert() {
        let text = "- une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = insert();
        let (note, _) = tabbed(&mut vim, &view(text, &parsed, 10), true);
        assert_eq!(note, "  - une idée\n");
        assert_eq!(vim.mode, Mode::Insert, "the session holds");
    }

    #[test]
    fn an_insert_session_start_rides_the_shift_its_text_took() {
        let text = "- une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            insert_from: Some(5),
            ..insert()
        };
        tabbed(&mut vim, &view(text, &parsed, 10), true);
        assert_eq!(vim.insert_from, Some(7), "the mark moved with its text");

        // a session that began on an earlier line is left where it is
        let text = "a\n- idée\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            insert_from: Some(0),
            ..insert()
        };
        tabbed(&mut vim, &view(text, &parsed, 8), true);
        assert_eq!(vim.insert_from, Some(0));
    }

    #[test]
    fn visual_tab_moves_every_selected_line_and_spends_the_selection() {
        let text = "- une\n- deux\n- trois\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Line),
            ..Vim::default()
        };
        let (note, caret) =
            tabbed(&mut vim, &spread(text, &parsed, 2, 8), true);
        assert_eq!(note, "  - une\n  - deux\n- trois\n");
        assert_eq!(caret, 12, "two lines' worth of spaces ahead of it");
        assert_eq!(vim.mode, Mode::Normal, "as vim's own > leaves it");
    }

    #[test]
    fn a_reversed_selection_covers_the_same_lines() {
        let text = "- une\n- deux\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        let (note, _) = tabbed(&mut vim, &spread(text, &parsed, 8, 2), true);
        assert_eq!(note, "  - une\n  - deux\n");
    }

    #[test]
    fn a_selection_across_a_block_keeps_the_separator_it_spans() {
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Line),
            ..Vim::default()
        };
        let parsed = blocks::segment(NOTE);
        let list = NOTE.find("- une").expect("the fixture has its list");
        let (note, _) =
            tabbed(&mut vim, &spread(NOTE, &parsed, 0, list), true);
        assert_eq!(
            note,
            "  = l'été\n\n  - une idée\n- deux cafés\n\nLa pluie, enfin arrivée.\n",
            "the blank line between the blocks rode along untouched"
        );
    }

    #[test]
    fn a_blank_line_inside_a_selection_stays_blank() {
        let text = "- une\n\n- deux\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Visual(VisualKind::Line),
            ..Vim::default()
        };
        let (note, _) = tabbed(&mut vim, &spread(text, &parsed, 0, 8), true);
        assert_eq!(note, "  - une\n\n  - deux\n", "no indent on nothing");
    }

    #[test]
    fn replace_mode_keeps_swallowing_tab() {
        let text = "- une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = Vim {
            mode: Mode::Replace,
            replace_from: Some(0),
            ..Vim::default()
        };
        assert_eq!(
            vim.handle(&Key::Tab, Modifiers::empty(), &view(text, &parsed, 3)),
            Outcome::Swallow,
        );
        assert_eq!(vim.mode, Mode::Replace, "the session holds");
    }

    #[test]
    fn a_tab_behind_an_armed_verb_aborts_it() {
        let text = "- une idée\n";
        let parsed = blocks::segment(text);
        for keys in ["d", "f"] {
            let mut vim = normal();
            feed(&mut vim, keys, &view(text, &parsed, 3));
            assert_eq!(
                vim.handle(
                    &Key::Tab,
                    Modifiers::empty(),
                    &view(text, &parsed, 3)
                ),
                Outcome::Swallow,
                "{keys} then tab"
            );
            assert_eq!(vim.operator, None, "{keys}");
            assert_eq!(vim.prefix, None, "{keys}");
        }
    }

    // -- the WORD siblings, % and * -----------------------------------------

    /// adr/2026-08-word-motions-add-their-big-siblings.md
    #[test]
    fn the_big_word_keys_answer_in_both_modes() {
        // "allons-y" is three small words and one WORD
        let text = "allons-y vite\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "W", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(9)]),
            "W crosses the hyphen w stops at"
        );
        assert_eq!(
            feed(&mut vim, "w", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(6)]),
        );
        assert_eq!(
            feed(&mut vim, "B", &view(text, &parsed, 9)),
            Outcome::Acts(vec![Act::Place(0)]),
        );
        assert_eq!(
            feed(&mut vim, "E", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(7)]),
        );
        // and the same three extend a selection rather than placing it
        let mut visual = Vim {
            mode: Mode::Visual(VisualKind::Char),
            ..Vim::default()
        };
        for (key, from, landing) in [("W", 0, 9), ("E", 0, 7), ("B", 9, 0)] {
            assert_eq!(
                feed(&mut visual, key, &spread(text, &parsed, from, from)),
                Outcome::Acts(vec![Act::Extend(landing)]),
                "{key} in visual",
            );
        }
        // % answers in visual too, over the same brackets
        let bracketed = "a (b) c\n";
        let parsed = blocks::segment(bracketed);
        assert_eq!(
            feed(&mut visual, "%", &spread(bracketed, &parsed, 0, 0)),
            Outcome::Acts(vec![Act::Extend(4)]),
        );
    }

    #[test]
    fn ge_and_g_uppercase_e_ride_the_g_prefix() {
        let text = "allons-y vite\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "ge", &view(text, &parsed, 9)),
            Outcome::Acts(vec![Act::Place(7)]),
            "the y, end of the small word before vite"
        );
        assert_eq!(
            feed(&mut vim, "gE", &view(text, &parsed, 9)),
            Outcome::Acts(vec![Act::Place(7)]),
            "the same y, which ends the WORD too"
        );
        assert_eq!(
            feed(&mut vim, "gE", &view(text, &parsed, 7)),
            Outcome::Acts(vec![Act::Place(0)]),
            "from inside the WORD there is nothing but the note's start"
        );
    }

    #[test]
    fn the_big_siblings_inherit_the_two_house_quirks() {
        // cw acts as ce on a word, and so must cW as cE
        let text = "allons-y vite\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "cW", &view(text, &parsed, 0))),
            Outcome::Acts(vec![
                Act::SetClipboard("allons-y".into()),
                Act::Splice {
                    span: 0..8,
                    text: String::new(),
                    caret: 0,
                },
            ]),
            "cW took the WORD, not its trailing blank"
        );
        assert_eq!(vim.mode, Mode::Insert);
        // and dW on a line's last WORD stops at the line's end
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "dW", &view(text, &parsed, 9))),
            Outcome::Acts(vec![
                Act::SetClipboard("vite".into()),
                Act::Splice {
                    span: 9..13,
                    text: String::new(),
                    caret: 8,
                },
            ]),
        );
    }

    #[test]
    fn the_big_word_object_answers_behind_a_verb() {
        let text = "prends l'idée, vite\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "diW", &view(text, &parsed, 10))),
            Outcome::Acts(vec![
                Act::SetClipboard("l'idée,".into()),
                Act::Splice {
                    span: 7..15,
                    text: String::new(),
                    caret: 7,
                },
            ]),
        );
    }

    /// adr/2026-08-percent-matches-vims-default-pairs.md
    #[test]
    fn percent_jumps_and_carries_its_bracket_under_a_verb() {
        let text = "a (b) c\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "%", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(4)]),
        );
        // inclusive, so d% takes the closing bracket too
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "d%", &view(text, &parsed, 2))),
            Outcome::Acts(vec![
                Act::SetClipboard("(b)".into()),
                Act::Splice {
                    span: 2..5,
                    text: String::new(),
                    caret: 2,
                },
            ]),
        );
        // a line with no bracket consumes the key and stays
        let bare = "rien\n";
        let parsed = blocks::segment(bare);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "%", &view(bare, &parsed, 0)),
            Outcome::Swallow,
        );
    }

    /// adr/2026-08-star-searches-whole-words.md
    #[test]
    fn star_searches_whole_words_and_n_keeps_walking_them() {
        let text = "le mot, un motif, et mot.\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "*", &view(text, &parsed, 3)),
            Outcome::Acts(vec![Act::Place(21)]),
            "the mot inside motif is not a whole word"
        );
        // n inherits the whole-word rule the * pattern was born with
        assert_eq!(
            feed(&mut vim, "n", &view(text, &parsed, 21)),
            Outcome::Acts(vec![Act::Place(3)]),
            "wrapping past motif, not into it"
        );
        // # is the same walk backward
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "#", &view(text, &parsed, 21)),
            Outcome::Acts(vec![Act::Place(3)]),
        );
    }

    #[test]
    fn star_with_no_word_left_on_the_line_consumes_its_key_and_stays() {
        let text = "!!\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "*", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
        // and a word with no second occurrence anywhere still fails the jump
        let alone = "seul.\n";
        let parsed = blocks::segment(alone);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "*", &view(alone, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(0)]),
            "one occurrence wraps onto itself, as vim's does"
        );
    }

    // -- J, gJ, and the case verbs ------------------------------------------

    /// adr/2026-08-join-walks-the-visible-rows.md
    #[test]
    fn j_joins_the_next_row_with_one_space() {
        let text = "une idée\n  et sa suite\nplus loin\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "J", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..23,
                text: "une idée et sa suite".into(),
                caret: 9,
            }]),
            "the second line's indent goes, one space stands in its place"
        );
        // the checkpoint is there, so one u takes the join back
        let mut vim = normal();
        assert!(matches!(
            feed(&mut vim, "J", &view(text, &parsed, 0)),
            Outcome::Acts(acts) if acts.first() == Some(&Act::Checkpoint)
        ));
    }

    #[test]
    fn a_count_joins_that_many_lines_and_gj_keeps_the_indent() {
        let text = "une idée\n  et sa suite\nplus loin\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "3J", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..33,
                text: "une idée et sa suite plus loin".into(),
                caret: 9,
            }]),
        );
        // gJ takes the next row exactly as it stands, indent and all
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "gJ", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..23,
                text: "une idée  et sa suite".into(),
                caret: 9,
            }]),
        );
    }

    #[test]
    fn join_obeys_vims_no_space_rules_and_stops_at_the_notes_end() {
        // A row that already ends in a blank takes no second one. It has
        // to be a row *inside* a multi-line construct: an ordinary line's
        // trailing spaces belong to the block separator, not to the row
        // (adr/2026-08-per-line-block-segmentation.md), so only a block
        // that spans lines can hand a join one.
        let text = "#f(a, \n   b)\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "J", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..12,
                text: "#f(a, b)".into(),
                caret: 6,
            }]),
            "no second space after the one already there"
        );
        let closing = "#f(a\n) et\n";
        let parsed = blocks::segment(closing);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "J", &view(closing, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..9,
                text: "#f(a) et".into(),
                caret: 4,
            }]),
            "a closing bracket takes no space before it"
        );
        // an empty row pulled up adds nothing, and the caret clamps back
        // onto a cluster rather than resting past the line's end
        let trailing = "mot\n\n";
        let parsed = blocks::segment(trailing);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "J", &view(trailing, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..4,
                text: "mot".into(),
                caret: 2,
            }]),
        );
        // the note's last row has nothing below it to pull up
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "J", &view(trailing, &parsed, trailing.len())),
            Outcome::Swallow,
        );
    }

    #[test]
    fn a_join_of_two_blank_rows_leaves_the_caret_at_the_start() {
        let text = "\n\n\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "J", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..1,
                text: String::new(),
                caret: 0,
            }]),
        );
    }

    #[test]
    fn the_dot_repeats_a_join() {
        let text = "une\ndeux\ntrois\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        feed(&mut vim, "J", &view(text, &parsed, 0));
        let joined = "une deux\ntrois\n";
        let reparsed = blocks::segment(joined);
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(joined, &reparsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..14,
                text: "une deux trois".into(),
                caret: 8,
            }]),
        );
    }

    /// adr/2026-08-case-operators-are-verbs.md
    #[test]
    fn the_case_verbs_take_a_motion_and_fill_no_register() {
        let text = "une idée noire\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "gUw", &view(text, &parsed, 0)),
            Outcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 0..4,
                    text: "UNE ".into(),
                    caret: 0,
                },
            ]),
            "no SetClipboard: a recasing verb moves no text"
        );
        let mut vim = normal();
        assert_eq!(
            stripped(feed(
                &mut vim,
                "guiw",
                &view("UNE IDÉE\n", &blocks::segment("UNE IDÉE\n"), 0)
            )),
            Outcome::Acts(vec![Act::Splice {
                span: 0..3,
                text: "une".into(),
                caret: 0,
            }]),
        );
        // g~ flips what it spans
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "g~w", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..4,
                text: "UNE ".into(),
                caret: 0,
            }]),
        );
    }

    #[test]
    fn the_case_verbs_double_onto_the_current_line_both_ways() {
        let text = "une idée\ndeux\n";
        let parsed = blocks::segment(text);
        // guu, whose second key is not a verb key
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "gUU", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..10,
                text: "UNE IDÉE\n".into(),
                caret: 0,
            }]),
        );
        // and gUgU, which reaches the same place through the g prefix
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "gUgU", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..10,
                text: "UNE IDÉE\n".into(),
                caret: 0,
            }]),
        );
        let mut vim = normal();
        assert_eq!(
            stripped(feed(
                &mut vim,
                "guu",
                &view("UNE\ndeux\n", &blocks::segment("UNE\ndeux\n"), 0)
            )),
            Outcome::Acts(vec![Act::Splice {
                span: 0..4,
                text: "une\n".into(),
                caret: 0,
            }]),
        );
        let mut vim = normal();
        assert_eq!(
            stripped(feed(&mut vim, "g~~", &view(text, &parsed, 0))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..10,
                text: "UNE IDÉE\n".into(),
                caret: 0,
            }]),
        );
    }

    #[test]
    fn a_case_verb_crossed_with_another_aborts_and_the_dot_replays_it() {
        let text = "une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "gUgu", &view(text, &parsed, 0)),
            Outcome::Swallow,
            "two different case verbs cancel, as d then c does"
        );
        assert_eq!(vim.operator, None, "the grammar recovered");
        // the dot replays a case change through the same resolution path
        let mut vim = normal();
        feed(&mut vim, "gUw", &view(text, &parsed, 0));
        let shouted = "UNE idée\n";
        let reparsed = blocks::segment(shouted);
        assert_eq!(
            stripped(feed(&mut vim, ".", &view(shouted, &reparsed, 4))),
            Outcome::Acts(vec![Act::Splice {
                span: 4..9,
                text: "IDÉE".into(),
                caret: 4,
            }]),
        );
    }

    // -- visual gains its verbs ---------------------------------------------

    fn visual(kind: VisualKind) -> Vim {
        Vim {
            mode: Mode::Visual(kind),
            ..Vim::default()
        }
    }

    /// adr/2026-08-visual-gains-p-r-s-and-gv.md
    #[test]
    fn visual_s_is_c_and_the_case_verbs_answer_over_the_selection() {
        let text = "une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = visual(VisualKind::Char);
        assert_eq!(
            stripped(feed(&mut vim, "s", &spread(text, &parsed, 0, 2))),
            Outcome::Acts(vec![
                Act::SetClipboard("une".into()),
                Act::Splice {
                    span: 0..3,
                    text: String::new(),
                    caret: 0,
                },
            ]),
            "s changes the selection exactly as c does"
        );
        assert_eq!(vim.mode, Mode::Insert);
        for (key, wanted) in [("U", "UNE"), ("u", "une"), ("~", "UNE")] {
            let mut vim = visual(VisualKind::Char);
            assert_eq!(
                stripped(feed(&mut vim, key, &spread(text, &parsed, 0, 2))),
                Outcome::Acts(vec![Act::Splice {
                    span: 0..3,
                    text: wanted.into(),
                    caret: 0,
                }]),
                "visual {key} recases and fills no register",
            );
            assert_eq!(vim.mode, Mode::Normal, "the selection is spent");
        }
    }

    #[test]
    fn visual_r_overwrites_every_cluster_and_keeps_the_newlines() {
        let text = "une\ndeux\n";
        let parsed = blocks::segment(text);
        let mut vim = visual(VisualKind::Line);
        assert_eq!(
            stripped(feed(&mut vim, "rx", &spread(text, &parsed, 0, 5))),
            Outcome::Acts(vec![Act::Splice {
                span: 0..9,
                text: "xxx\nxxxx\n".into(),
                caret: 0,
            }]),
            "the line breaks stand, or the lines would merge"
        );
        assert_eq!(vim.mode, Mode::Normal);
        // a selection standing on the note's real trailing empty line has
        // nothing to overwrite, and checkpoints nothing
        // (adr/2026-08-cursor-always-in-the-note.md)
        let mut vim = visual(VisualKind::Char);
        assert_eq!(
            feed(&mut vim, "rx", &spread(text, &parsed, 9, 9)),
            Outcome::Acts(vec![Act::Place(9)]),
        );
    }

    #[test]
    fn visual_p_replaces_the_span_without_clobbering_the_clipboard() {
        let text = "une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = visual(VisualKind::Char);
        assert_eq!(
            feed(&mut vim, "p", &spread(text, &parsed, 0, 2)),
            Outcome::Acts(vec![
                Act::Checkpoint,
                Act::PasteOver {
                    span: 0..3,
                    linewise: false,
                },
            ]),
            "no SetClipboard: what it replaced does not go back out"
        );
        assert_eq!(vim.mode, Mode::Normal);
        // a V selection keeps its line-wise shape
        let mut vim = visual(VisualKind::Line);
        assert_eq!(
            feed(&mut vim, "p", &spread(text, &parsed, 0, 2)),
            Outcome::Acts(vec![
                Act::Checkpoint,
                Act::PasteOver {
                    span: 0..10,
                    linewise: true,
                },
            ]),
        );
    }

    #[test]
    fn gv_puts_the_last_selection_back() {
        let text = "une idée\n";
        let parsed = blocks::segment(text);
        let mut vim = visual(VisualKind::Char);
        // the selection the mode dies with is the one the exiting keystroke
        // saw — here, Escape's
        vim.handle(
            &Key::Escape,
            Modifiers::empty(),
            &spread(text, &parsed, 0, 4),
        );
        assert_eq!(vim.mode, Mode::Normal);
        assert_eq!(
            feed(&mut vim, "gv", &view(text, &parsed, 0)),
            Outcome::Acts(vec![Act::Place(0), Act::Extend(4)]),
        );
        assert_eq!(vim.mode, Mode::Visual(VisualKind::Char));
        // with nothing remembered, gv consumes its keys and stays
        let mut fresh = normal();
        assert_eq!(
            feed(&mut fresh, "gv", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
    }

    /// adr/2026-08-objects-reach-visual-mode.md
    #[test]
    fn objects_reach_visual_mode_and_take_the_selection() {
        let text = "prends « une idée » vite\n";
        let parsed = blocks::segment(text);
        let mut vim = visual(VisualKind::Char);
        // viw over "une": the head rests on the run's last cluster
        assert_eq!(
            feed(&mut vim, "iw", &spread(text, &parsed, 10, 10)),
            Outcome::Acts(vec![Act::Place(10), Act::Extend(12)]),
        );
        assert_eq!(vim.mode, Mode::Visual(VisualKind::Char), "still visual");
        // a« takes the guillemets with it
        let mut vim = visual(VisualKind::Char);
        assert_eq!(
            feed(&mut vim, "a«", &spread(text, &parsed, 10, 10)),
            Outcome::Acts(vec![Act::Place(7), Act::Extend(20)]),
        );
        // a line-wise selection takes the object's raw ends
        let mut vim = visual(VisualKind::Line);
        assert_eq!(
            feed(&mut vim, "iw", &spread(text, &parsed, 10, 10)),
            Outcome::Acts(vec![Act::Place(10), Act::Extend(13)]),
        );
        // an object that names nothing consumes its keys and stays
        let mut vim = visual(VisualKind::Char);
        assert_eq!(
            feed(&mut vim, "i(", &spread(text, &parsed, 10, 10)),
            Outcome::Swallow,
        );
        assert_eq!(vim.mode, Mode::Visual(VisualKind::Char));
        // and an unknown object key aborts the prefix
        let mut vim = visual(VisualKind::Char);
        assert_eq!(
            feed(&mut vim, "iz", &spread(text, &parsed, 10, 10)),
            Outcome::Swallow,
        );
        assert_eq!(vim.prefix, None, "the grammar recovered");
    }

    #[test]
    fn an_object_key_with_no_verb_and_no_selection_aborts() {
        let text = "une idée\n";
        let parsed = blocks::segment(text);
        // reached only through the dot's replay path, which resolves an
        // object noun without arming the operator first
        let mut vim = normal();
        vim.prefix = Some(Prefix::Object { around: false });
        assert_eq!(
            feed(&mut vim, "w", &view(text, &parsed, 0)),
            Outcome::Swallow
        );
        assert_eq!(vim.prefix, None, "the grammar recovered");
    }

    // -- zz zt zb, gf ------------------------------------------------------

    /// adr/2026-08-scroll-anchor-is-consumed-once.md
    #[test]
    fn the_z_prefix_names_where_the_caret_sits_and_binds_nothing_else() {
        let text = "une idée\n";
        let parsed = blocks::segment(text);
        for (keys, anchor) in [
            ("zz", Anchor::Center),
            ("zt", Anchor::Top),
            ("zb", Anchor::Bottom),
        ] {
            let mut vim = normal();
            assert_eq!(
                feed(&mut vim, keys, &view(text, &parsed, 0)),
                Outcome::Acts(vec![Act::Scroll(anchor)]),
                "{keys}",
            );
        }
        // this editor has no folds: za and zR stay inert rather than
        // pretending to be something
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "za", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
        assert_eq!(vim.prefix, None, "the grammar recovered");
        // and it answers in visual too
        let mut vim = visual(VisualKind::Char);
        assert_eq!(
            feed(&mut vim, "zz", &spread(text, &parsed, 0, 2)),
            Outcome::Acts(vec![Act::Scroll(Anchor::Center)]),
        );
    }

    /// adr/2026-08-gf-follows-the-link.md
    #[test]
    fn gf_asks_the_executor_to_follow_the_link_under_the_caret() {
        let text = "voir #l(\"une-idee\")\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "gf", &view(text, &parsed, 10)),
            Outcome::Acts(vec![Act::FollowLink]),
        );
        // behind a verb, f is the find prefix and g's own f means nothing
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, "dgf", &view(text, &parsed, 0)),
            Outcome::Swallow,
        );
    }

    // -- the ex line --------------------------------------------------------

    /// The three-line fixture the substitutes work over: the middle row is
    /// the caret's, and "vieux" stands on two of the three.
    const EX: &str = "un vieux mot\nun vieux vieux\nrien\n";

    fn ex(vim: &mut Vim, line: &str, at: usize) -> ExOutcome {
        let parsed = blocks::segment(EX);
        vim.commit_ex(line, &view(EX, &parsed, at))
    }

    /// adr/2026-08-ex-line-is-literal-and-global.md
    #[test]
    fn the_colon_opens_the_prompt_and_visual_spells_its_range() {
        let parsed = blocks::segment(EX);
        let mut vim = normal();
        assert_eq!(
            feed(&mut vim, ":", &view(EX, &parsed, 0)),
            Outcome::Acts(vec![Act::OpenEx {
                prefill: String::new()
            }]),
        );
        let mut vim = visual(VisualKind::Line);
        assert_eq!(
            feed(&mut vim, ":", &spread(EX, &parsed, 0, 14)),
            Outcome::Acts(vec![Act::OpenEx {
                prefill: "'<,'>".into()
            }]),
        );
        assert_eq!(
            vim.mode,
            Mode::Visual(VisualKind::Line),
            "the selection lives until the line is submitted (AIR ERR-6)"
        );
    }

    #[test]
    fn an_empty_line_does_nothing_and_w_saves() {
        let mut vim = normal();
        assert_eq!(ex(&mut vim, "", 0), ExOutcome::Acts(Vec::new()));
        assert_eq!(ex(&mut vim, "   ", 0), ExOutcome::Acts(Vec::new()));
        assert_eq!(ex(&mut vim, "w", 0), ExOutcome::Acts(vec![Act::Save]),);
    }

    #[test]
    fn a_bare_number_goes_to_that_line() {
        let mut vim = normal();
        assert_eq!(
            ex(&mut vim, "2", 0),
            ExOutcome::Acts(vec![Act::Place(13)]),
            "one-based, and landing on the first non-blank"
        );
        // out of range says so rather than moving somewhere arbitrary
        let ExOutcome::Refused(reason) = ex(&mut vim, "42", 0) else {
            panic!("a line past the note's end must refuse");
        };
        assert!(reason.contains("past the note's 4 lines"), "{reason}");
        assert!(reason.contains("the caret stayed"), "{reason}");
        let ExOutcome::Refused(zero) = ex(&mut vim, "0", 0) else {
            panic!("there is no line zero");
        };
        assert!(zero.contains("past the note's"), "{zero}");
    }

    #[test]
    fn substitute_replaces_every_hit_on_the_caret_line() {
        let mut vim = normal();
        // gdefault: no g needed, and a g is accepted and ignored
        for spelled in ["s/vieux/neuf/", "s/vieux/neuf/g", "s/vieux/neuf"] {
            assert_eq!(
                ex(&mut vim, spelled, 13),
                ExOutcome::Acts(vec![
                    Act::Checkpoint,
                    Act::Splice {
                        span: 13..27,
                        text: "un neuf neuf".into(),
                        caret: 13,
                    },
                ]),
                "{spelled}",
            );
        }
    }

    #[test]
    fn substitute_takes_whatever_delimiter_follows_the_s() {
        let mut vim = normal();
        assert_eq!(
            ex(&mut vim, "s#vieux#ne/uf#", 0),
            ExOutcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 0..12,
                    text: "un ne/uf mot".into(),
                    caret: 0,
                },
            ]),
            "a replacement carrying a slash needs another delimiter"
        );
    }

    #[test]
    fn substitute_ranges_span_the_note_the_rows_and_the_selection() {
        let mut vim = normal();
        // % walks the whole note, and the bytes between rows ride along
        assert_eq!(
            ex(&mut vim, "%s/vieux/neuf/", 0),
            ExOutcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 0..33,
                    text: "un neuf mot\nun neuf neuf\nrien\n".into(),
                    // the last row it touched, measured in the *new* text:
                    // row 0 shrank by one byte on the way past
                    caret: 12,
                },
            ]),
            "the caret lands on the last row it touched"
        );
        // an explicit row window, in either order
        for spelled in ["1,1s/vieux/neuf/", "1s/vieux/neuf/"] {
            assert_eq!(
                ex(&mut vim, spelled, 20),
                ExOutcome::Acts(vec![
                    Act::Checkpoint,
                    Act::Splice {
                        span: 0..12,
                        text: "un neuf mot".into(),
                        caret: 0,
                    },
                ]),
                "{spelled}",
            );
        }
        // and the visual range, which reads the live anchor and head
        let parsed = blocks::segment(EX);
        let mut vim = visual(VisualKind::Line);
        assert_eq!(
            vim.commit_ex("'<,'>s/vieux/neuf/", &spread(EX, &parsed, 20, 3)),
            ExOutcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 0..27,
                    text: "un neuf mot\nun neuf neuf".into(),
                    caret: 12,
                },
            ]),
        );
    }

    #[test]
    fn a_reversed_or_overshot_row_range_still_names_a_window() {
        let mut vim = normal();
        // the order the two ends are spelled in does not matter
        assert_eq!(
            ex(&mut vim, "3,1s/vieux/neuf/", 0),
            ex(&mut vim, "1,3s/vieux/neuf/", 0),
        );
        // and a row past the note's end clamps onto its last, rather than
        // refusing the way a bare :99 does — the range named a window, and
        // the window exists
        assert_eq!(
            ex(&mut vim, "1,99s/vieux/neuf/", 0),
            ex(&mut vim, "%s/vieux/neuf/", 0),
        );
    }

    /// A submitted command line that fails says so — it must never fall
    /// silent the way an unbound keystroke does (AIR ERR-2).
    #[test]
    fn every_refusal_says_what_happened_and_what_to_type_instead() {
        let mut vim = normal();
        let cases = [
            ("nope", "unknown command"),
            ("sort", "unknown command"),
            ("s", "needs a pattern"),
            ("s//neuf/", "needs a pattern"),
            ("s/absent/neuf/", "found no \"absent\""),
            ("%", "names a range with no command"),
            ("'<,'>", "names a range with no command"),
        ];
        for (spelled, wanted) in cases {
            let ExOutcome::Refused(reason) = ex(&mut vim, spelled, 0) else {
                panic!("\":{spelled}\" must refuse");
            };
            assert!(reason.contains(wanted), "\":{spelled}\" said {reason:?}");
            // every one of them names a way forward, not just a complaint
            assert!(
                reason.contains(" — ") || reason.contains("spell it"),
                "\":{spelled}\" said {reason:?}",
            );
        }
    }

    #[test]
    fn substitute_folds_case_by_the_same_smartcase_rule_the_slash_uses() {
        let text = "Un Vieux vieux\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        // all-lowercase matches both
        assert_eq!(
            vim.commit_ex("s/vieux/neuf/", &view(text, &parsed, 0)),
            ExOutcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 0..14,
                    text: "Un neuf neuf".into(),
                    caret: 0,
                },
            ]),
        );
        // a capital makes it exact
        assert_eq!(
            vim.commit_ex("s/Vieux/neuf/", &view(text, &parsed, 0)),
            ExOutcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 0..14,
                    text: "Un neuf vieux".into(),
                    caret: 0,
                },
            ]),
        );
    }

    #[test]
    fn overlapping_hits_are_consumed_left_to_right() {
        let text = "aaaa\n";
        let parsed = blocks::segment(text);
        let mut vim = normal();
        assert_eq!(
            vim.commit_ex("s/aa/b/", &view(text, &parsed, 0)),
            ExOutcome::Acts(vec![
                Act::Checkpoint,
                Act::Splice {
                    span: 0..4,
                    text: "bb".into(),
                    caret: 0,
                },
            ]),
            "two hits, not three: nothing inside a hit starts another"
        );
    }
}
