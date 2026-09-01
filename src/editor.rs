use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::blocks::{self, Block};
use crate::caret;

/// The edit-command layer over one open note: the buffer, its block map,
/// the active block, the caret and the trouble the shell forwards to the
/// status surface. The widget only forwards events here — the v2 modal
/// keymap slots in between the two without touching either (adr/2026-07-hybrid-active-block-textarea.md).
#[derive(Debug, Default)]
pub struct Editor {
    buffer: Option<Buffer>,
    blocks: Vec<Block>,
    active: Option<usize>,
    /// App-owned caret in note-global bytes, meaningful only while a block
    /// is active; selection is `anchor != head`
    /// (adr/2026-08-caret-on-editor-note-bytes.md).
    caret: Caret,
    /// The column a run of vertical moves holds through short lines,
    /// counted in grapheme clusters; any other move forgets it.
    goal: Option<usize>,
    /// Vim-grain undo: whole-note snapshots, one per change intent —
    /// checkpointed by the grammar, never per keystroke
    /// (adr/2026-08-undo-at-vim-grain.md). Dies with the editor, so each
    /// open note carries its own history.
    history: Vec<Snapshot>,
    undone: Vec<Snapshot>,
    /// What went wrong since the shell last asked: deposited by the paths
    /// that discover it (internal chains like activate → deactivate → flush
    /// included), drained by the shell's forwarding effect and reported to
    /// `status` — the editor detects, the status surface displays
    /// (adr/2026-08-status-surface-owns-notices.md).
    trouble: Option<Trouble>,
}

/// The editor's failure modes, typed so the status surface can rank them:
/// a diverged widget edit, a note that would not open, a save that failed —
/// the last two carrying typed text that is not on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trouble {
    /// Every guard in `edit` failing means the widget diverged from the
    /// buffer — a bug, surfaced as a visible notice rather than silently
    /// eaten input.
    Stale,
    Open(String),
    Save(String),
    /// The file changed on disk under the open buffer: the save refused to
    /// clobber the other author, and only the user can pick a side
    /// (adr/2026-08-external-edit-conflict-commands.md).
    Conflict,
}

/// One undo step: the whole note and where the caret stood — notes are
/// small, and whole-text snapshots make undo trivially correct.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    head: usize,
}

/// A runaway grammar cannot eat the memory: the oldest steps fall off.
const UNDO_DEPTH: usize = 100;

/// The caret both ends of a selection describe: `head` is where it blinks
/// and moves, `anchor` where the selection began. Collapsed when equal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caret {
    pub anchor: usize,
    pub head: usize,
}

/// What a deletion keystroke removes when nothing is selected; a selection
/// is always removed whole, whichever key asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deletion {
    Back,
    Forward,
    WordBack,
}

/// `offset` clamped into the text and back onto a char boundary — a stale
/// coordinate degrades instead of panicking.
fn floor_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    (0..=offset)
        .rev()
        .find(|&at| text.is_char_boundary(at))
        .unwrap_or(0)
}

impl Editor {
    /// A closed editor: the empty-day state, and the fallback when a note
    /// cannot be opened.
    pub fn closed() -> Editor {
        Editor::default()
    }

    /// Opens `file` and segments it; a file that cannot be read becomes a
    /// closed editor carrying the error as its trouble.
    pub fn open(file: PathBuf) -> Editor {
        match Buffer::open(file.clone()) {
            Ok(note) => {
                let blocks = blocks::segment(note.text());
                let text = note.text().to_string();
                let end = text.len();
                Editor {
                    // an open note always has its cursor somewhere: the
                    // last block wakes active with the caret on its last
                    // line (adr/2026-08-cursor-always-in-the-note.md)
                    active: Some(blocks.len().saturating_sub(1)),
                    blocks,
                    buffer: Some(note),
                    caret: Caret {
                        anchor: end,
                        head: end,
                    },
                    // the opened file is the first undo step: even a
                    // session that never leaves insert can fall back to it
                    history: vec![Snapshot {
                        text: text.clone(),
                        head: end,
                    }],
                    ..Editor::default()
                }
            }
            Err(err) => Editor {
                trouble: Some(Trouble::Open(format!(
                    "{}: {err}",
                    file.display()
                ))),
                ..Editor::default()
            },
        }
    }

    /// The open note's path and current text — what fragment rendering
    /// needs, as one option so the two can never disagree.
    pub fn note(&self) -> Option<(&Path, &str)> {
        self.buffer.as_ref().map(|note| (note.file(), note.text()))
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// The undrained trouble — the forwarding effect's gate, read before it
    /// commits to a draining write.
    pub fn trouble(&self) -> Option<&Trouble> {
        self.trouble.as_ref()
    }

    /// Drains the trouble for the status surface: the editor detects, the
    /// shell forwards, status displays — nothing here renders
    /// (adr/2026-08-status-surface-owns-notices.md).
    pub fn take_trouble(&mut self) -> Option<Trouble> {
        self.trouble.take()
    }

    /// One widget edit: the active block's whole new content, spliced into
    /// the buffer (the trailing separator is not the widget's to touch),
    /// with every later block shifted by the delta. No reparse — block
    /// boundaries move only on activate/deactivate.
    pub fn edit(&mut self, value: &str) {
        let (Some(index), Some(note)) = (self.active, self.buffer.as_mut())
        else {
            self.trouble = Some(Trouble::Stale);
            return;
        };
        let Some(block) = self.blocks.get(index) else {
            self.trouble = Some(Trouble::Stale);
            return;
        };
        if note.replace_range(block.content(), value) {
            blocks::resize(&mut self.blocks, index, value.len());
        } else {
            self.trouble = Some(Trouble::Stale);
        }
    }

    /// Every insertion the widget makes — typing, Enter, paste, a committed
    /// composition, an accepted link completion: `text` replaces the
    /// selection (or lands at the collapsed caret) and the caret ends after
    /// it. Routed through `edit` — the block's whole new content, one
    /// staleness policy (adr/2026-08-ctrl-l-link-picker.md).
    pub fn insert_at_caret(&mut self, text: &str) {
        let text = nbsp_folded(text);
        let Some((content, source)) = self.active_slice() else {
            self.trouble = Some(Trouble::Stale);
            return;
        };
        let (anchor, head) = self.caret_in_block();
        let span = anchor.min(head)..anchor.max(head);
        if !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.end)
        {
            self.trouble = Some(Trouble::Stale);
            return;
        }
        let mut value = source;
        value.replace_range(span.clone(), &text);
        // `active_slice` proved the block fits the buffer, which is the
        // same validity `edit` re-checks — the splice cannot refuse here
        self.edit(&value);
        self.place(content.start + span.start + text.len());
    }

    /// One typed cluster, the only insertion that closes its own pair.
    /// Paste, the IME's commit and the link picker keep going through
    /// `insert_at_caret`, which never pairs: text someone already balanced
    /// must not be balanced twice
    /// (adr/2026-08-autopairs-in-the-typing-path.md).
    pub fn insert_typed(&mut self, cluster: &str) {
        let Some((content, source)) = self.active_slice() else {
            self.insert_at_caret(cluster);
            return;
        };
        let (anchor, head) = self.caret_in_block();
        if anchor != head {
            // a selection is replaced, never wrapped — wrapping a span is
            // what visual S is for
            self.insert_at_caret(cluster);
            return;
        }
        let before = source.get(..head).unwrap_or_default();
        let after = source.get(head..).unwrap_or_default();
        if let Some(over) = close_through(cluster, after) {
            self.place(content.start + head + over);
            return;
        }
        let Some((open, close)) = opening_pair(cluster, before, after) else {
            self.insert_at_caret(cluster);
            return;
        };
        // `splice` re-checks the boundary this offset assumed
        let at = content.start + head;
        self.splice(at..at, &format!("{open}{close}"), at + open.len());
    }

    /// Enter continues a `-`/`+` list line at its own indent (a fresh
    /// marker for a non-empty item, the line rewritten one level out for
    /// an empty one); every other press remains an ordinary newline. `> `
    /// is no longer expanded here — it is the stored quote syntax the
    /// template itself reads, so Enter on such a line falls through to
    /// this same list check and, finding no marker, inserts a plain
    /// newline.
    pub fn insert_newline(&mut self) {
        let Some((content, source)) = self.active_slice() else {
            self.insert_at_caret("\n");
            return;
        };
        let (anchor, head) = self.caret_in_block();
        let at_line_end = source
            .get(head..)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('\n'));
        if anchor != head || !at_line_end {
            self.insert_at_caret("\n");
            return;
        }
        let line_start = source[..head].rfind('\n').map_or(0, |at| at + 1);
        let line = &source[line_start..head];
        let span = content.start + line_start..content.start + head;
        match list_continuation(line) {
            Continuation::Item(next) => self.insert_at_caret(&next),
            Continuation::Close(rest) => {
                let caret = span.start + rest.len();
                self.splice(span, &rest, caret);
            }
            Continuation::None => self.insert_at_caret("\n"),
        }
    }

    /// Ctrl+T's line toggle: a plain line is prefixed into a fresh
    /// unchecked item, a `-`/`+` item is promoted straight to unchecked,
    /// and `[ ]`/`[x]` flip in place — the checkbox itself is never
    /// removed (adr/2026-08-ctrl-t-toggles-the-todo.md). Buffer-side, not
    /// the grammar's: what edits text as you type belongs to the editor.
    pub fn toggle_todo(&mut self) {
        let Some((content, source)) = self.active_slice() else {
            self.trouble = Some(Trouble::Stale);
            return;
        };
        let (_, head) = self.caret_in_block();
        let line_start = source[..head].rfind('\n').map_or(0, |at| at + 1);
        let line_end = source[head..]
            .find('\n')
            .map_or(source.len(), |at| head + at);
        let line = &source[line_start..line_end];
        let replacement = todo_toggled(line);
        let span = content.start + line_start..content.start + line_end;
        // every transition in `todo_toggled` only grows or keeps the
        // line's byte length, so this cannot underflow
        let caret = content.start + head + (replacement.len() - line.len());
        // the chord fires from normal mode, with no insert session to have
        // already checkpointed for it — one change intent, one undo step
        // (adr/2026-08-ctrl-t-toggles-the-todo.md)
        self.checkpoint();
        self.splice(span, &replacement, caret);
    }

    /// A deletion keystroke: the selection when one exists, otherwise the
    /// cluster or word the key names. At the block's edge with nothing to
    /// remove, nothing happens — blocks join by being emptied, never by
    /// backspacing across the hidden separator.
    pub fn delete_at_caret(&mut self, kind: Deletion) {
        let Some((content, source)) = self.active_slice() else {
            self.trouble = Some(Trouble::Stale);
            return;
        };
        let (anchor, head) = self.caret_in_block();
        let span = if anchor != head {
            anchor.min(head)..anchor.max(head)
        } else {
            match kind {
                Deletion::Back => back_span(&source, head),
                Deletion::Forward => head..caret::next_cluster(&source, head),
                Deletion::WordBack => caret::word_left(&source, head)..head,
            }
        };
        if span.start == span.end
            || !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.end)
        {
            return;
        }
        let mut value = source;
        value.replace_range(span.clone(), "");
        self.edit(&value);
        self.place(content.start + span.start);
    }

    /// One caret movement. Vertical moves keep the goal column and, on the
    /// block's edge lines, slide to the neighbouring block through the same
    /// flush-and-resegment path as a click — unless the move extends a
    /// selection, which stays inside the active block like the widget it
    /// replaced. A plain horizontal arrow over a selection collapses it to
    /// the matching edge, the way the old textarea's did.
    pub fn move_caret(&mut self, motion: caret::Move, select: bool) {
        let Some((content, source)) = self.active_slice() else {
            return;
        };
        let Caret { anchor, head } = self.caret;
        let rel = head.clamp(content.start, content.end) - content.start;
        let collapse = !select && anchor != head;
        let mut goal = None;
        let target = match motion {
            caret::Move::Left if collapse => {
                anchor.min(head).clamp(content.start, content.end)
                    - content.start
            }
            caret::Move::Right if collapse => {
                anchor.max(head).clamp(content.start, content.end)
                    - content.start
            }
            caret::Move::Left => caret::prev_cluster(&source, rel),
            caret::Move::Right => caret::next_cluster(&source, rel),
            caret::Move::LineStart => caret::line_start(&source, rel),
            caret::Move::LineEnd => caret::line_end(&source, rel),
            caret::Move::WordLeft => caret::word_left(&source, rel),
            caret::Move::WordRight => caret::word_right(&source, rel),
            caret::Move::Up | caret::Move::Down => {
                let up = motion == caret::Move::Up;
                match caret::vertical(&source, rel, up, self.goal) {
                    Some((offset, column)) => {
                        goal = Some(column);
                        offset
                    }
                    None if !select && self.can_slide(up) => {
                        self.slide(up);
                        return;
                    }
                    // the note's edge, or a selection that must not leave
                    // the block: clamp to the block's ends, the textarea's
                    // own edge-line behaviour
                    None if up => 0,
                    None => source.len(),
                }
            }
        };
        self.goal = goal;
        self.caret.head = content.start + target;
        if !select {
            self.caret.anchor = self.caret.head;
        }
    }

    /// A mouse press answered by the hit probe: `piece_start` is the
    /// clicked span's block-relative start, `units` the UTF-16 offset the
    /// webview measured within it. A press past the text (or a probe miss
    /// signalled as `usize::MAX`) lands at the block's end.
    pub fn place_in_block(
        &mut self,
        piece_start: usize,
        units: usize,
        select: bool,
    ) {
        let Some((content, source)) = self.active_slice() else {
            return;
        };
        let start = piece_start.min(source.len());
        let rel = source
            .get(start..)
            .map(|rest| start + blocks::byte_offset_of_utf16(rest, units))
            .unwrap_or(source.len());
        self.goal = None;
        self.caret.head = content.start + rel;
        if !select {
            self.caret.anchor = self.caret.head;
        }
    }

    /// Ctrl+A, block-scoped like the widget it replaced: the whole active
    /// block's content, caret at its end.
    pub fn select_all(&mut self) {
        let Some(content) = self.active_content() else {
            return;
        };
        self.caret = Caret {
            anchor: content.start,
            head: content.end,
        };
        self.goal = None;
    }

    /// A click on a rendered block: flush any pending edit, resegment (the
    /// edit may have split or merged blocks), then land on the block owning
    /// the clicked block's first byte — a coordinate, so it survives the
    /// index shuffle resegmentation can cause. The caret lands at the woken
    /// block's end, where the old textarea's default put it.
    pub fn activate(&mut self, start: usize) {
        self.deactivate();
        self.active = (!self.blocks.is_empty())
            .then(|| blocks::block_at(&self.blocks, start));
        let end = self
            .active_content()
            .map(|content| content.end)
            .unwrap_or(0);
        self.caret = Caret {
            anchor: end,
            head: end,
        };
        self.goal = None;
    }

    /// One change intent begins: the grammar checkpoints the state it is
    /// about to change — before an operator's splice, once on entering
    /// insert — never per keystroke (adr/2026-08-undo-at-vim-grain.md).
    pub fn checkpoint(&mut self) {
        let Some((_, text)) = self.note() else { return };
        if self
            .history
            .last()
            .is_some_and(|snapshot| snapshot.text == text)
        {
            return;
        }
        self.history.push(Snapshot {
            text: text.to_string(),
            head: self.caret.head,
        });
        if self.history.len() > UNDO_DEPTH {
            self.history.remove(0);
        }
        self.undone.clear();
    }

    /// u: back one change intent. Checkpoints equal to the present are
    /// stepped over, so an entered-then-abandoned insert session costs no
    /// press.
    pub fn undo(&mut self) {
        let Some((_, current)) = self.note() else {
            return;
        };
        let current = current.to_string();
        let stale = self
            .history
            .iter()
            .rev()
            .take_while(|snapshot| snapshot.text == current)
            .count();
        self.history.truncate(self.history.len() - stale);
        let Some(previous) = self.history.pop() else {
            return;
        };
        self.undone.push(Snapshot {
            text: current,
            head: self.caret.head,
        });
        self.restore(previous);
    }

    /// Ctrl+R: forward again, until a new change intent clears the path.
    pub fn redo(&mut self) {
        let Some(next) = self.undone.pop() else {
            return;
        };
        let Some((_, current)) = self.note() else {
            return;
        };
        self.history.push(Snapshot {
            text: current.to_string(),
            head: self.caret.head,
        });
        self.restore(next);
    }

    /// The whole note becomes the snapshot: splice, resegment, wake the
    /// caret's block — deactivation's mechanism, like every cross-block
    /// change (adr/2026-08-editor-splice-cross-block.md). Undo and redo
    /// proved the buffer is there, and a whole-range splice cannot refuse.
    fn restore(&mut self, snapshot: Snapshot) {
        self.buffer = self.buffer.take().map(|mut note| {
            let whole = 0..note.text().len();
            let _ = note.replace_range(whole, &snapshot.text);
            note
        });
        self.blocks = blocks::segment(&snapshot.text);
        self.active = (!self.blocks.is_empty())
            .then(|| blocks::block_at(&self.blocks, snapshot.head));
        self.place(floor_boundary(&snapshot.text, snapshot.head));
    }

    /// p and P once the clipboard answered: `motions::paste_spec` decides
    /// the insertion pure, and the splice lands it — an insertion inside
    /// the active block, always (adr/2026-08-one-register-the-clipboard.md).
    pub fn paste(&mut self, clip: &str, before: bool, count: usize) {
        let clip = nbsp_folded(clip);
        let Some(at) = self.caret() else { return };
        let (span, body, caret) = {
            let Some((_, text)) = self.note() else { return };
            crate::motions::paste_spec(
                text,
                &self.blocks,
                at.head,
                &clip,
                before,
                count,
            )
        };
        self.splice(span, &body, caret);
    }

    /// A grammar change, note-global: a span inside the active block's
    /// content routes through `edit` — the typing path, no resegment, so a
    /// blank line still splits at the next resegmentation — while a span
    /// that crosses the block splices the whole buffer and resegments, the
    /// deactivation mechanism (adr/2026-08-editor-splice-cross-block.md).
    /// The caret lands at `caret`, a post-splice coordinate.
    pub fn splice(
        &mut self,
        span: Range<usize>,
        replacement: &str,
        caret: usize,
    ) {
        let Some(content) = self.active_content() else {
            self.trouble = Some(Trouble::Stale);
            return;
        };
        if span.start <= span.end
            && span.start >= content.start
            && span.end <= content.end
        {
            let Some(source) = self.active_source().map(str::to_string) else {
                self.trouble = Some(Trouble::Stale);
                return;
            };
            let rel = span.start - content.start..span.end - content.start;
            if !source.is_char_boundary(rel.start)
                || !source.is_char_boundary(rel.end)
            {
                self.trouble = Some(Trouble::Stale);
                return;
            }
            let mut value = source;
            value.replace_range(rel, replacement);
            self.edit(&value);
            self.place(caret);
            return;
        }
        let Some(note) = self.buffer.as_mut() else {
            self.trouble = Some(Trouble::Stale);
            return;
        };
        if !note.replace_range(span, replacement) {
            self.trouble = Some(Trouble::Stale);
            return;
        }
        let text = self
            .buffer
            .as_ref()
            .map(|note| note.text().to_string())
            .unwrap_or_default();
        self.blocks = blocks::segment(&text);
        self.active = (!self.blocks.is_empty())
            .then(|| blocks::block_at(&self.blocks, caret));
        self.place(floor_boundary(&text, caret));
    }

    /// A motion's landing: the caret collapses to a note-global offset,
    /// waking the block that owns it when it lies outside the active one —
    /// the same flush-and-resegment path as a click, with the coordinate
    /// riding through because resegmenting never edits
    /// (adr/2026-08-caret-on-editor-note-bytes.md). Phase 5's search lands
    /// through this too.
    pub fn place_at(&mut self, offset: usize) {
        let offset = self.land(offset);
        self.place(offset);
    }

    /// Visual mode's motion: the head moves — waking blocks exactly as a
    /// landing does — while the anchor holds the selection's far end
    /// (adr/2026-08-visual-selection-is-the-anchor.md).
    pub fn extend_to(&mut self, offset: usize) {
        let anchor = self.caret.anchor;
        let offset = self.land(offset);
        self.caret = Caret {
            anchor,
            head: offset,
        };
        self.goal = None;
    }

    /// o in visual mode: the caret jumps to the selection's other end.
    pub fn swap_ends(&mut self) {
        let Caret { anchor, head } = self.caret;
        let landed = self.land(anchor);
        self.caret = Caret {
            anchor: head,
            head: landed,
        };
        self.goal = None;
    }

    /// The shared landing: floor the coordinate onto a boundary and wake
    /// the block owning it when the caret leaves the active one.
    fn land(&mut self, offset: usize) -> usize {
        let Some((_, text)) = self.note() else {
            return offset;
        };
        let offset = floor_boundary(text, offset);
        let inside = self.active_content().is_some_and(|content| {
            offset >= content.start && offset <= content.end
        });
        if !inside {
            self.activate(offset);
        }
        offset
    }

    /// A vertical move leaving the block: the neighbouring block wakes
    /// through the same flush-and-resegment path as a click; at the note's
    /// ends, nothing happens.
    pub fn slide(&mut self, up: bool) {
        let Some(index) = self.active else { return };
        let target = match up {
            true if index > 0 => self.blocks[index - 1].range.start,
            false if index + 1 < self.blocks.len() => {
                self.blocks[index + 1].range.start
            }
            _ => return,
        };
        self.activate(target);
    }

    /// Escape or clicking away: save, resegment, back to fully rendered. A
    /// failed save deposits its trouble — the text survives in the buffer.
    pub fn deactivate(&mut self) {
        self.flush();
        self.blocks = self
            .buffer
            .as_ref()
            .map(|note| blocks::segment(note.text()))
            .unwrap_or_default();
        self.active = None;
    }

    /// The way back in: the block owning the remembered caret wakes and the
    /// caret lands exactly where deactivation left it — the caret survives
    /// on `Editor` even while nothing is active
    /// (adr/2026-08-enter-returns-to-the-note.md). A closed editor has no
    /// block to wake and stays rendered.
    pub fn reactivate(&mut self) {
        self.place_at(self.caret.head);
    }

    /// Ctrl+Q and deactivation: save and surface the outcome. Returns
    /// whether the note reached disk, so a failed flush can cancel a quit
    /// instead of losing the buffer
    /// (adr/2026-07-ctrl-q-flushes-then-closes.md). A failure deposits; a
    /// success never clears a pending deposit — an undrained `Stale` from
    /// the same gesture must still reach the shell, and resolution is the
    /// status surface's job, not this one's.
    pub fn flush(&mut self) -> bool {
        match self.save() {
            None => true,
            Some(trouble) => {
                self.trouble = Some(trouble);
                false
            }
        }
    }

    /// The autosave tick: saves the open note without touching editor
    /// state. The caller decides how to surface a failure — the autosave
    /// resource must not write the signal it subscribes to unguarded, or it
    /// would restart itself forever.
    pub fn save(&self) -> Option<Trouble> {
        self.buffer.as_ref().and_then(|note| match note.save() {
            Ok(()) => None,
            Err(SaveError::Conflict) => Some(Trouble::Conflict),
            Err(SaveError::Io(err)) => Some(Trouble::Save(format!(
                "{}: {err}",
                note.file().display()
            ))),
        })
    }

    /// Keep-mine, one side of the conflict's fork: the buffer overwrites
    /// the diverged file and the guard re-arms on the new version. Returns
    /// whether it landed; a disk that refuses deposits like a flush.
    pub fn clobber(&mut self) -> bool {
        let failure = self.buffer.as_ref().and_then(|note| {
            note.clobber()
                .err()
                .map(|err| format!("{}: {err}", note.file().display()))
        });
        match failure {
            None => true,
            Some(detail) => {
                self.trouble = Some(Trouble::Save(detail));
                false
            }
        }
    }

    /// The active block's own text, or `None` when there is no active block
    /// or its span no longer fits the buffer — the widget diverged.
    pub fn active_source(&self) -> Option<&str> {
        let block = self.blocks.get(self.active?)?;
        let (_, text) = self.note()?;
        text.get(block.content())
    }

    /// The caret, only while a block is active — a rendered note has no
    /// caret to draw or move.
    pub fn caret(&self) -> Option<Caret> {
        self.active.map(|_| self.caret)
    }

    /// The caret as the widget draws it: block-relative bytes, clamped into
    /// the active block's content so a stale coordinate degrades to the
    /// block's edge instead of panicking downstream.
    pub fn caret_in_block(&self) -> (usize, usize) {
        self.active_content()
            .map(|content| {
                let clamp = |offset: usize| {
                    offset.clamp(content.start, content.end) - content.start
                };
                (clamp(self.caret.anchor), clamp(self.caret.head))
            })
            .unwrap_or((0, 0))
    }

    /// The selected note-global span, `None` when collapsed or inactive.
    pub fn selection(&self) -> Option<Range<usize>> {
        self.active?;
        let Caret { anchor, head } = self.caret;
        (anchor != head).then(|| anchor.min(head)..anchor.max(head))
    }

    /// What Ctrl+C copies: the selected text, `None` when nothing is.
    pub fn selected_text(&self) -> Option<String> {
        let span = self.selection()?;
        let (_, text) = self.note()?;
        text.get(span).map(str::to_string)
    }

    /// The active block's content span and text together — every caret op's
    /// first read, as one option so they can never disagree.
    fn active_slice(&self) -> Option<(Range<usize>, String)> {
        let content = self.active_content()?;
        let source = self.active_source()?.to_string();
        Some((content, source))
    }

    /// The active block's content range in note-global bytes.
    fn active_content(&self) -> Option<Range<usize>> {
        self.blocks.get(self.active?).map(Block::content)
    }

    /// Whether a neighbouring block exists in that direction.
    fn can_slide(&self, up: bool) -> bool {
        self.active.is_some_and(|index| {
            if up {
                index > 0
            } else {
                index + 1 < self.blocks.len()
            }
        })
    }

    /// The collapsed caret after an edit: both ends at `head`, the goal
    /// column forgotten.
    fn place(&mut self, head: usize) {
        self.caret = Caret { anchor: head, head };
        self.goal = None;
    }
}

/// The no-break space a keystroke never means. On the `ca` layout `[` is
/// AltGr+`^` and U+00A0 is AltGr+Space, so holding AltGr through `- [ ] `
/// types the brackets around a no-break space; the template's checklist
/// rule matches a `space` element and never a `text` one, and the item
/// renders as a literal bullet. Folding on the way into the buffer keeps
/// the note plain ASCII where a space was meant
/// (adr/2026-08-nbsp-folded-on-buffer-entry.md).
fn nbsp_folded(text: &str) -> String {
    text.replace('\u{a0}', " ")
}

/// The pairs that close themselves as you type: key, opening text,
/// closing text, and whether both ends are the same character (which needs
/// the apostrophe guard). Typst's `*` and `_` and the `<` `>` of a
/// comparison punctuate prose far more often than they nest, so they stay
/// out of the set the surround keys carry; the guillemets keep the padding
/// those keys already chose (adr/2026-08-autopairs-in-the-typing-path.md,
/// adr/2026-08-surround-pair-set-and-padding.md).
const PAIRS: [(&str, &str, &str, bool); 7] = [
    ("(", "(", ")", false),
    ("[", "[", "]", false),
    ("{", "{", "}", false),
    ("«", "« ", " »", false),
    ("'", "'", "'", true),
    ("\"", "\"", "\"", true),
    ("`", "`", "`", true),
];

/// Typing the closing half of a pair the caret already sits inside steps
/// over it instead of doubling it — over the guillemet's padding space too.
/// The bytes to step, or `None` for a cluster that closes nothing waiting.
fn close_through(cluster: &str, after: &str) -> Option<usize> {
    PAIRS
        .iter()
        .find(|(_, _, close, _)| close.trim_start() == cluster)
        .filter(|(_, _, close, _)| after.starts_with(*close))
        .map(|(_, _, close, _)| close.len())
}

/// The pair a typed cluster opens. A quote-like against a word character
/// on either side opens nothing: in a French vault `'` is an apostrophe far
/// more often than a delimiter, and `l'ami` must stay `l'ami`.
fn opening_pair(
    cluster: &str,
    before: &str,
    after: &str,
) -> Option<(&'static str, &'static str)> {
    let (_, open, close, quoting) =
        PAIRS.iter().find(|(key, ..)| *key == cluster)?;
    let apostrophe = *quoting
        && (before
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
            || after.chars().next().is_some_and(char::is_alphanumeric));
    (!apostrophe).then_some((*open, *close))
}

/// What one Backspace erases: both halves when the caret sits between a
/// pair this typing closed, the guillemets' padding included, and the
/// ordinary preceding cluster everywhere else.
fn back_span(source: &str, head: usize) -> Range<usize> {
    let before = source.get(..head).unwrap_or_default();
    let after = source.get(head..).unwrap_or_default();
    PAIRS
        .iter()
        .find(|(_, open, close, _)| {
            before.ends_with(*open) && after.starts_with(*close)
        })
        .map_or_else(
            || caret::prev_cluster(source, head)..head,
            |(_, open, close, _)| head - open.len()..head + close.len(),
        )
}

/// What Enter at the end of a list item does
/// (adr/2026-08-list-continuation-on-enter.md).
enum Continuation {
    /// Text to insert at the caret: the newline, the indentation, the
    /// marker the next item repeats.
    Item(String),
    /// The line rewritten in place, with no newline at all: an empty item
    /// walks one level out per press and, at the margin, leaves a bare
    /// line rather than orphan alignment spaces.
    Close(String),
    /// Not a list line — Enter stays an ordinary newline.
    None,
}

fn list_continuation(line: &str) -> Continuation {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let Some((marker, used)) = list_marker(&line[indent..]) else {
        return Continuation::None;
    };
    if !line[indent + used..].trim().is_empty() {
        return Continuation::Item(format!("\n{}{marker}", &line[..indent]));
    }
    Continuation::Close(match indent {
        0 => String::new(),
        _ => format!(
            "{}{marker}",
            " ".repeat(indent.saturating_sub(caret::INDENT.len()))
        ),
    })
}

/// The marker a line carries and the bytes it spends, or `None` when what
/// follows is prose. Typst wants a space after the marker, so `-abc` is not
/// an item while a bare `-` closing the line is an empty one — the shape
/// the daily template seeds. A done item opens a fresh empty one: no item
/// is born already checked.
fn list_marker(rest: &str) -> Option<(&'static str, usize)> {
    let (marker, next) = [
        ("- [ ]", "- [ ] "),
        ("- [x]", "- [ ] "),
        ("-", "- "),
        ("+", "+ "),
    ]
    .into_iter()
    .find(|(marker, _)| {
        rest.strip_prefix(marker)
            .is_some_and(|tail| tail.is_empty() || tail.starts_with(' '))
    })?;
    Some((next, marker.len()))
}

/// The line Ctrl+T's todo toggle turns it into. `- [ ]` and `- [x]` swap
/// directly with the tail untouched; a `-`/`+` item is promoted to an
/// unchecked one, at most one marker-following space folded into the
/// canonical `- [ ] `; anything else — prose, an empty line — is prefixed
/// into a fresh unchecked item. Every branch only grows or keeps the
/// line's byte length, never shrinks it, which `toggle_todo` relies on
/// for its caret math.
fn todo_toggled(line: &str) -> String {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let body = if let Some(tail) = marker_tail(rest, "- [ ]") {
        format!("- [x]{tail}")
    } else if let Some(tail) = marker_tail(rest, "- [x]") {
        format!("- [ ]{tail}")
    } else if let Some(tail) = marker_tail(rest, "-") {
        format!("- [ ] {}", tail.strip_prefix(' ').unwrap_or(tail))
    } else if let Some(tail) = marker_tail(rest, "+") {
        format!("- [ ] {}", tail.strip_prefix(' ').unwrap_or(tail))
    } else {
        format!("- [ ] {rest}")
    };
    format!("{}{body}", &line[..indent])
}

/// A marker's tail when `rest` opens with it followed by a space or
/// nothing — the same boundary `list_marker` checks, so `-abc` reads as
/// prose rather than a truncated item.
fn marker_tail<'a>(rest: &'a str, marker: &str) -> Option<&'a str> {
    let tail = rest.strip_prefix(marker)?;
    (tail.is_empty() || tail.starts_with(' ')).then_some(tail)
}

#[derive(Debug)]
pub struct Buffer {
    file: PathBuf,
    text: String,
    /// The mtime of the version this buffer last read or wrote — the
    /// external-edit guard's reference point. A `Cell`, because the
    /// autosave tick holds the editor through a read-only `peek` and the
    /// stamp is bookkeeping no render ever depends on.
    stamp: std::cell::Cell<Option<std::time::SystemTime>>,
}

/// Why a save did not land: the disk refused, or the guard did — the file
/// changed under the buffer and clobbering it would erase another author
/// (adr/2026-08-external-edit-conflict-commands.md).
#[derive(Debug)]
pub enum SaveError {
    Conflict,
    Io(std::io::Error),
}

impl Buffer {
    pub fn open(file: PathBuf) -> Result<Buffer, std::io::Error> {
        // stamped before the read: an edit slipping between the two makes
        // the next save refuse instead of silently missing it
        let stamp = std::fs::metadata(&file)
            .and_then(|meta| meta.modified())
            .ok();
        let text = std::fs::read_to_string(&file)?;
        Ok(Buffer {
            file,
            text,
            stamp: std::cell::Cell::new(stamp),
        })
    }
    pub fn file(&self) -> &Path {
        &self.file
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    /// The hybrid editor's one edit operation: the widget hands back a whole
    /// block's new text, the buffer splices it into the note
    /// (adr/2026-07-hybrid-active-block-textarea.md). A reversed span or one
    /// off a char boundary is a stale caller — `String::replace_range` would
    /// panic there, so the edit is refused instead, and the refusal is
    /// returned rather than swallowed so the widget can surface it.
    #[must_use]
    pub fn replace_range(
        &mut self,
        span: Range<usize>,
        replacement: &str,
    ) -> bool {
        let valid = span.start <= span.end
            && self.text.is_char_boundary(span.start)
            && self.text.is_char_boundary(span.end);
        if valid {
            self.text.replace_range(span, replacement);
        }
        valid
    }
    /// The guarded save: refused when the file diverged from the stamp, so
    /// an edit made outside the app survives until the user picks a side.
    pub fn save(&self) -> Result<(), SaveError> {
        if self.diverged() {
            return Err(SaveError::Conflict);
        }
        self.clobber().map_err(SaveError::Io)
    }
    /// The write itself, guard skipped — keep-mine, and every save the
    /// guard waved through. Atomic (adr/2026-08-atomic-persist-seam.md),
    /// and the stamp re-arms on the version it just wrote.
    pub fn clobber(&self) -> Result<(), std::io::Error> {
        crate::persist::write_atomic(&self.file, &self.text)
            .map(|stamp| self.stamp.set(Some(stamp)))
    }
    /// Whether the file changed under the buffer. A file that vanished is
    /// not divergence — recreating the user's own text erases no one — and
    /// a stamp that never resolved leaves the guard off rather than
    /// blocking every save.
    fn diverged(&self) -> bool {
        let disk = std::fs::metadata(&self.file)
            .and_then(|meta| meta.modified())
            .ok();
        disk.zip(self.stamp.get())
            .is_some_and(|(disk, known)| disk != known)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::io::ErrorKind;

    use super::*;

    /// Saves are refused by locking the *directory*: an atomic write never
    /// opens the target file, it creates a sibling and renames — so only
    /// the directory can say no. Callers unlock before the tempdir drops.
    fn lock(dir: &Path, readonly: bool) {
        let mut permissions = std::fs::metadata(dir)
            .expect("the dir exists")
            .permissions();
        permissions.set_readonly(readonly);
        std::fs::set_permissions(dir, permissions)
            .expect("the dir permissions are set");
    }

    /// An external edit the guard must notice: rewrite the file and push
    /// its mtime to the epoch, so the divergence never hides inside the
    /// filesystem's timestamp granularity.
    fn edit_behind(file: &Path, text: &str) {
        std::fs::write(file, text).expect("the outside edit is written");
        std::fs::OpenOptions::new()
            .write(true)
            .open(file)
            .expect("the note reopens for backdating")
            .set_modified(std::time::SystemTime::UNIX_EPOCH)
            .expect("the mtime is set");
    }

    #[test]
    fn open_reads_the_text_and_keeps_the_path_it_was_given() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "= a title\n").expect("the note is written");

        let buffer = Buffer::open(file.clone()).expect("the note opens");
        assert_eq!(buffer.text(), "= a title\n");
        assert_eq!(buffer.file(), file);
    }

    #[test]
    fn open_reports_a_file_that_is_not_there() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let error = Buffer::open(dir.path().join("missing.typ")).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound, "{error}");
    }

    #[test]
    fn open_refuses_bytes_that_are_not_utf8() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, [0xff, 0xfe]).expect("the bytes are written");

        let error = Buffer::open(file).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData, "{error}");
    }

    #[test]
    fn replace_range_splices_and_save_round_trips_it() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "one two three\n").expect("the note is written");

        let mut buffer = Buffer::open(file.clone()).expect("the note opens");
        assert!(buffer.replace_range(4..7, "deux"));
        assert_eq!(buffer.text(), "one deux three\n");

        // nothing reaches disk until save is called
        assert_eq!(
            std::fs::read_to_string(&file).expect("the note is readable"),
            "one two three\n"
        );

        buffer.save().expect("the note saves");
        let reopened = Buffer::open(file).expect("the note reopens");
        assert_eq!(reopened.text(), "one deux three\n");
    }

    #[test]
    fn replace_range_covers_the_edges() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "abc").expect("the note is written");

        let mut buffer = Buffer::open(file).expect("the note opens");
        assert!(buffer.replace_range(0..1, "A"));
        assert!(buffer.replace_range(2..3, "C"));
        assert_eq!(buffer.text(), "AbC");
        assert!(buffer.replace_range(0..3, ""), "the whole text can go");
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn stale_spans_are_refused_and_the_text_survives() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "été\n").expect("the note is written");

        let mut buffer = Buffer::open(file).expect("the note opens");
        assert!(!buffer.replace_range(0..9, "x"), "past the end");
        let reversed = Range { start: 3, end: 2 };
        assert!(!buffer.replace_range(reversed, "x"), "reversed");
        assert!(!buffer.replace_range(1..4, "x"), "mid-char start");
        assert!(!buffer.replace_range(0..4, "x"), "mid-char end");
        assert_eq!(buffer.text(), "été\n", "a refused edit changes nothing");
    }

    // -- the Editor: the edit-command layer over the buffer ------------------

    const NOTE: &str = "#import \"/templates/template.typ\": *\n\
                        #show: note\n\
                        #meta(id: \"x\")\n\
                        \n\
                        = title\n\
                        \n\
                        prose\n";

    fn open_note(text: &str) -> (tempfile::TempDir, Editor) {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, text).expect("the note is written");
        (dir, Editor::open(file))
    }

    #[test]
    fn open_segments_the_note_and_wakes_its_last_block() {
        // an open note always has its cursor somewhere
        // (adr/2026-08-cursor-always-in-the-note.md)
        let (_dir, editor) = open_note(NOTE);
        assert_eq!(editor.blocks().len(), 6, "{:?}", editor.blocks());
        assert_eq!(editor.active(), Some(5));
        assert_eq!(editor.trouble(), None);
        let (file, text) = editor.note().expect("the note is open");
        assert!(file.ends_with("note.typ"));
        assert_eq!(text, NOTE);
    }

    #[test]
    fn open_failure_is_a_closed_editor_carrying_the_trouble() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let mut editor = Editor::open(dir.path().join("absente.typ"));
        assert!(editor.note().is_none());
        assert!(editor.blocks().is_empty());
        match editor.take_trouble() {
            Some(Trouble::Open(detail)) => {
                assert!(detail.contains("absente.typ"), "{detail}");
            }
            other => panic!("the failure is the trouble: {other:?}"),
        }
    }

    #[test]
    fn closed_is_empty_and_take_trouble_drains_once() {
        let mut editor = Editor::closed();
        assert!(editor.note().is_none());
        assert_eq!(editor.take_trouble(), None, "nothing to drain");

        let dir = tempfile::tempdir().expect("a temp dir is available");
        let mut editor = Editor::open(dir.path().join("absente.typ"));
        assert!(editor.trouble().is_some(), "the gate sees the deposit");
        assert!(editor.take_trouble().is_some(), "the drain takes it");
        assert_eq!(editor.trouble(), None, "and it is gone");
    }

    #[test]
    fn activate_lands_on_the_clicked_block_and_edit_splices() {
        let (_dir, mut editor) = open_note(NOTE);
        let start = editor.blocks()[2].range.start;
        editor.activate(start);
        assert_eq!(editor.active(), Some(2));

        editor.edit("= new title");
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= new title"), "{text}");
        assert!(text.ends_with("prose\n"), "later blocks survive: {text}");
        assert_eq!(
            editor.blocks().last().expect("a block").range.end,
            text.len(),
            "later spans shifted with the edit"
        );
    }

    #[test]
    fn deactivate_saves_resegments_and_clears_the_active_block() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[4].range.start);
        // a blank line typed inside the block splits it on deactivate
        editor.edit("prose\n\nencore\n");
        editor.deactivate();

        assert_eq!(editor.active(), None);
        assert_eq!(editor.trouble(), None);
        // "prose" splits into "prose", a blank line and "encore", each its
        // own block, plus the note's own trailing empty line
        assert_eq!(editor.blocks().len(), 9, "{:?}", editor.blocks());
        let saved = std::fs::read_to_string(dir.path().join("note.typ"))
            .expect("the note is readable");
        assert!(saved.contains("encore"), "{saved}");
    }

    #[test]
    fn activate_flushes_the_previous_block_before_moving() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        editor.edit("= renamed");
        editor.activate(editor.blocks()[4].range.start);

        assert_eq!(editor.active(), Some(4));
        let saved = std::fs::read_to_string(dir.path().join("note.typ"))
            .expect("the note is readable");
        assert!(saved.contains("= renamed"), "the move saved: {saved}");
    }

    #[test]
    fn a_failed_save_becomes_the_trouble_and_the_text_survives() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        editor.edit("= unsaved");

        lock(dir.path(), true);
        editor.deactivate();
        lock(dir.path(), false);
        match editor.take_trouble() {
            Some(Trouble::Save(detail)) => {
                assert!(detail.contains("note.typ"), "{detail}");
            }
            other => panic!("the save failure is deposited: {other:?}"),
        }
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= unsaved"), "nothing was lost: {text}");
    }

    #[test]
    fn edits_against_a_stale_editor_are_dropped_loudly() {
        // no block active: only deactivation reaches it now
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.edit("anything");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // an active index the block map no longer has
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks.clear();
        editor.edit("anything");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // a span the buffer refuses
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks[0].content_end = NOTE.len() + 40;
        editor.edit("anything");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, NOTE, "a refused edit changes nothing");
    }

    #[test]
    fn emptying_a_blocks_content_merges_it_away() {
        // the separator is not the widget's to touch, so joining paragraphs
        // works by emptying one: the bare separator left behind is absorbed
        // at the next resegmentation. A per-line block that empties does
        // not vanish from the map though — it becomes just another blank
        // line, same as its neighbours (adr/2026-08-per-line-block-segmentation.md)
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        editor.edit("");
        editor.deactivate();
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("prose"), "the neighbours survive: {text}");
        assert!(!text.contains("title"), "the emptied block is gone: {text}");
    }

    // -- the caret: app-owned, note-global bytes -----------------------------
    // (adr/2026-08-caret-on-editor-note-bytes.md)

    #[test]
    fn open_lands_the_caret_on_the_notes_last_line() {
        let (_dir, editor) = open_note(NOTE);
        let caret = editor.caret().expect("an open note has a caret");
        assert_eq!(caret.head, NOTE.len());
        assert_eq!(caret.anchor, NOTE.len(), "collapsed");
        assert_eq!(
            editor.caret_in_block(),
            (0, 0),
            "the note's own trailing empty line"
        );
    }

    #[test]
    fn activation_lands_the_caret_at_the_woken_blocks_end() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        let content = editor.blocks()[2].content();
        let caret = editor.caret().expect("a caret");
        assert_eq!(caret.head, content.end);
        assert_eq!(editor.caret_in_block(), (7, 7), "= title is seven bytes");
    }

    #[test]
    fn a_rendered_note_has_no_caret() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        assert_eq!(editor.caret(), None);
        assert_eq!(editor.caret_in_block(), (0, 0));
        assert_eq!(editor.selection(), None);
        assert_eq!(editor.selected_text(), None);
        assert_eq!(Editor::closed().caret(), None);
    }

    #[test]
    fn insert_at_caret_types_and_the_caret_follows() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        editor.move_caret(caret::Move::LineStart, false);
        editor.move_caret(caret::Move::Right, false);
        editor.move_caret(caret::Move::Right, false);
        editor.insert_at_caret("é");
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= étitle"), "{text}");
        assert!(text.ends_with("prose\n"), "later blocks survive: {text}");
        assert_eq!(editor.caret_in_block(), (4, 4), "after the é");
        assert_eq!(editor.trouble(), None);
    }

    #[test]
    fn insert_at_caret_folds_the_altgr_no_break_space() {
        // `[` is AltGr+`^` on the ca layout and U+00A0 is AltGr+Space, so
        // a checklist typed without letting AltGr go carries a no-break
        // space the template's rule never matches
        // (adr/2026-08-nbsp-folded-on-buffer-entry.md)
        // a per-line block's content stops before a line's own trailing
        // space (the parser groups it with the newline into one Space
        // node — adr/2026-08-per-line-block-segmentation.md), so the
        // marker's own space is typed here rather than pre-seeded
        let (_dir, mut editor) = open_note("-\n");
        editor.activate(0);
        editor.place_at(1);
        for key in [" ", "[", "\u{a0}", "]"] {
            editor.insert_at_caret(key);
        }
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "- [ ]\n");
        // the caret counts the folded byte, not the two the no-break
        // space would have spent
        assert_eq!(editor.caret_in_block(), (5, 5), "after the ]");
        assert_eq!(editor.trouble(), None);
    }

    #[test]
    fn insert_at_caret_replaces_the_selection() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[4].range.start);
        editor.move_caret(caret::Move::LineStart, false);
        for _ in 0..5 {
            editor.move_caret(caret::Move::Right, true);
        }
        assert_eq!(editor.selected_text().as_deref(), Some("prose"));
        editor.insert_at_caret("vers");
        let (_, text) = editor.note().expect("still open");
        assert!(text.ends_with("vers\n"), "{text}");
        assert_eq!(editor.selection(), None, "typing collapsed it");
    }

    #[test]
    fn enter_on_a_greater_than_line_is_an_ordinary_newline() {
        // `> ` is the stored quote syntax now, read by the template's
        // `show par:` rule at compile time — Enter no longer rewrites it,
        // so the literal `>` stays on disk (adr/2026-08-greater-than-
        // expands-to-quote.md superseded).
        let (_dir, mut editor) = open_note("> La vie est belle");

        editor.insert_newline();

        let (_, text) = editor.note().expect("the note stays open");
        assert_eq!(text, "> La vie est belle\n");
        assert_eq!(editor.trouble(), None);
    }

    // -- the todo toggle: Ctrl+T flips the caret's line's checkbox ---------
    // (adr/2026-08-ctrl-t-toggles-the-todo.md)

    #[test]
    fn todo_toggled_covers_every_branch() {
        for (line, expected) in [
            ("prose", "- [ ] prose"),
            ("", "- [ ] "),
            ("  prose indented", "  - [ ] prose indented"),
            ("- item", "- [ ] item"),
            ("-", "- [ ] "),
            ("+ item", "- [ ] item"),
            ("- [ ] item", "- [x] item"),
            ("- [x] item", "- [ ] item"),
        ] {
            assert_eq!(todo_toggled(line), expected, "for {line:?}");
        }
    }

    #[test]
    fn toggle_todo_prefixes_a_plain_line_and_rides_the_caret() {
        let (_dir, mut editor) = open_note("prose");

        editor.toggle_todo();

        let (_, text) = editor.note().expect("the note stays open");
        assert_eq!(text, "- [ ] prose");
        assert_eq!(editor.caret_in_block(), (11, 11));
        assert_eq!(editor.trouble(), None);
    }

    #[test]
    fn toggle_todo_never_removes_a_checkbox_once_it_has_one() {
        let (_dir, mut editor) = open_note("- [x] done");
        editor.place_at(0);

        editor.toggle_todo();
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "- [ ] done");

        editor.toggle_todo();
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "- [x] done", "back where it started, never plain");
    }

    #[test]
    fn toggle_todo_finds_the_line_inside_a_multi_line_block() {
        // a raw fence is the one construct a block never splits
        // (adr/2026-08-per-line-block-segmentation.md), so it is the one
        // shape left where the caret's line sits strictly between two
        // newlines inside a single block's own content — every other
        // block is now one physical line, where a line's own start or
        // end is the block's too
        let (_dir, mut editor) = open_note("```\nprose\nplus\n```\n");
        editor.activate(0);
        editor.place_at(12); // inside "plus", the fence's middle line

        editor.toggle_todo();

        let (_, text) = editor.note().expect("the note stays open");
        assert_eq!(text, "```\nprose\n- [ ] plus\n```\n");
    }

    #[test]
    fn toggle_todo_against_an_inactive_editor_is_stale() {
        let (_dir, mut editor) = open_note("prose");
        editor.deactivate();

        editor.toggle_todo();

        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
    }

    #[test]
    fn toggle_todo_checkpoints_its_own_undo_step() {
        // the chord fires from normal mode, with no insert session already
        // holding a checkpoint for it — one `u` must undo only the toggle,
        // never the paragraph typed before it too
        // (adr/2026-08-ctrl-t-toggles-the-todo.md)
        let (_dir, mut editor) = open_note("un\n");
        editor.activate(0);
        // intent one: `i`, type, Escape — insert entry's own checkpoint
        editor.checkpoint();
        editor.place_at(2);
        editor.insert_at_caret(" mot");

        editor.place_at(0);
        editor.toggle_todo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "- [ ] un mot\n");

        editor.undo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un mot\n", "the toggle alone reverted");
        editor.undo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un\n", "the typed paragraph reverted next");
    }

    // -- autopairs: the typing path closes what it opens -------------------
    // (adr/2026-08-autopairs-in-the-typing-path.md)

    #[test]
    fn every_open_delimiter_types_its_own_close() {
        for (typed, expected, caret) in [
            ("(", "()", 1),
            ("[", "[]", 1),
            ("{", "{}", 1),
            ("«", "«  »", 3),
            ("'", "''", 1),
            ("\"", "\"\"", 1),
            ("`", "``", 1),
        ] {
            let (_dir, mut editor) = open_note("");
            editor.insert_typed(typed);
            let (_, text) = editor.note().expect("still open");
            assert_eq!(text, expected, "typing {typed}");
            // the guillemet's caret lands inside its padding, after `« `
            assert_eq!(editor.caret_in_block(), (caret, caret), "{typed}");
            assert_eq!(editor.trouble(), None);
        }
    }

    #[test]
    fn typing_a_close_steps_over_the_one_waiting() {
        for (open, close, expected) in [
            ("(", ")", "()"),
            ("[", "]", "[]"),
            ("{", "}", "{}"),
            ("«", "»", "«  »"),
            ("\"", "\"", "\"\""),
        ] {
            let (_dir, mut editor) = open_note("");
            editor.insert_typed(open);
            editor.insert_typed(close);
            let (_, text) = editor.note().expect("still open");
            assert_eq!(text, expected, "{open} then {close} doubled nothing");
            let end = expected.len();
            assert_eq!(editor.caret_in_block(), (end, end), "{close}");
        }
    }

    #[test]
    fn a_close_with_nothing_waiting_is_typed_plainly() {
        let (_dir, mut editor) = open_note("");
        editor.insert_typed(")");
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, ")", "no pair to step over");
    }

    #[test]
    fn a_french_apostrophe_stays_one_character() {
        // `'` is an apostrophe far more often than a delimiter here
        let (_dir, mut editor) = open_note("");
        for cluster in ["l", "'", "a", "m", "i"] {
            editor.insert_typed(cluster);
        }
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l'ami");
    }

    #[test]
    fn a_quote_against_a_word_on_either_side_opens_nothing() {
        // before: the apostrophe case; after: quoting an existing word
        let (_dir, mut editor) = open_note("mot");
        editor.place_at(0);
        editor.insert_typed("\"");
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "\"mot", "the word ahead kept the quote single");

        let (_dir, mut editor) = open_note("mot");
        editor.insert_typed("\"");
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "mot\"", "and the word behind too");
    }

    #[test]
    fn a_bracket_pairs_against_a_word_where_a_quote_would_not() {
        let (_dir, mut editor) = open_note("l");
        editor.insert_typed("(");
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l()", "brackets never carry the apostrophe doubt");
    }

    #[test]
    fn typing_over_a_selection_replaces_it_without_pairing() {
        let (_dir, mut editor) = open_note("mot");
        editor.place_at(0);
        editor.extend_to(3);
        editor.insert_typed("(");
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "(", "a selection is replaced, never wrapped");
    }

    #[test]
    fn paste_and_the_ime_commit_never_pair() {
        // the regression that proves `insert_typed` is a separate door
        let (_dir, mut editor) = open_note("");
        editor.insert_at_caret("#l(");
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "#l(", "already-balanced text is not rebalanced");
    }

    #[test]
    fn typing_against_a_stale_editor_is_dropped_loudly() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.insert_typed("(");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
    }

    #[test]
    fn backspace_between_a_pair_takes_both_halves() {
        for (typed, left) in [("(", ""), ("«", ""), ("\"", "")] {
            let (_dir, mut editor) = open_note("");
            editor.insert_typed(typed);
            editor.delete_at_caret(Deletion::Back);
            let (_, text) = editor.note().expect("still open");
            assert_eq!(text, left, "backspacing the {typed} pair");
        }
    }

    #[test]
    fn backspace_beside_a_pair_takes_one_cluster_as_ever() {
        // the caret is past the close, not between the halves
        let (_dir, mut editor) = open_note("()");
        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "(", "only the cluster behind went");
    }

    // -- list continuation: Enter repeats the marker ------------------------
    // (adr/2026-08-list-continuation-on-enter.md)

    #[test]
    fn enter_repeats_the_marker_of_a_written_item() {
        for (line, expected) in [
            ("- une idée", "- une idée\n- "),
            ("+ une idée", "+ une idée\n+ "),
            ("- [ ] écrire", "- [ ] écrire\n- [ ] "),
            // no item is born already checked
            ("- [x] écrit", "- [x] écrit\n- [ ] "),
            ("  - niché", "  - niché\n  - "),
        ] {
            let (_dir, mut editor) = open_note(line);
            editor.insert_newline();
            let (_, text) = editor.note().expect("still open");
            assert_eq!(text, expected, "after {line}");
            assert_eq!(
                editor.caret_in_block(),
                (expected.len(), expected.len()),
                "the caret follows the new marker"
            );
        }
    }

    #[test]
    fn enter_on_an_empty_item_walks_one_level_out_then_clears_the_line() {
        let (_dir, mut editor) = open_note("- alpha\n    - ");

        editor.insert_newline();
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "- alpha\n  - ", "one level out, no newline");

        editor.insert_newline();
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "- alpha\n- ", "another level");

        editor.insert_newline();
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "- alpha\n", "the line goes bare — no stray spaces");
        assert_eq!(editor.caret_in_block(), (8, 8), "at the margin");
    }

    #[test]
    fn the_templates_bare_checklist_marker_continues_and_closes() {
        // daily.typ seeds `- [ ]` with no trailing space
        let (_dir, mut editor) = open_note("- [ ]");
        editor.insert_newline();
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "", "an empty item at the margin ends the list");
    }

    #[test]
    fn a_marker_without_its_space_is_prose() {
        for line in ["-abc", "+abc", "prose", ""] {
            let (_dir, mut editor) = open_note(line);
            editor.insert_newline();
            let (_, text) = editor.note().expect("still open");
            assert_eq!(text, format!("{line}\n"), "{line} is not an item");
        }
    }

    #[test]
    fn enter_inside_an_item_still_splits_it_plainly() {
        let (_dir, mut editor) = open_note("- une idée");
        editor.place_at(5);
        editor.insert_newline();
        let (_, text) = editor.note().expect("still open");
        assert_eq!(
            text, "- une\n idée",
            "away from the end, an ordinary split"
        );
    }

    #[test]
    fn newline_against_a_closed_editor_is_dropped_loudly() {
        let mut editor = Editor::closed();

        editor.insert_newline();

        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
    }

    #[test]
    fn insert_at_caret_against_a_stale_editor_is_dropped_loudly() {
        // no block active: only deactivation reaches it now
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.insert_at_caret("x");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // an active index the block map no longer has
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks.clear();
        editor.insert_at_caret("x");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // a caret off a char boundary is a stale coordinate
        let (_dir, mut editor) = open_note("été\n");
        editor.activate(0);
        editor.caret = Caret { anchor: 1, head: 1 };
        editor.insert_at_caret("x");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "été\n", "a refused insert changes nothing");

        // a block whose span no longer fits the buffer
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks[0].content_end = NOTE.len() + 40;
        editor.insert_at_caret("x");
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, NOTE, "a refused insert changes nothing");
    }

    #[test]
    fn the_caret_reads_guard_each_divergence_alone() {
        // every widget read survives an editor whose halves diverged —
        // unreachable through the widget, but a future caller must not be
        // able to make them panic
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        assert_eq!(editor.active_source(), None, "no active block");

        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks.clear();
        assert_eq!(editor.active_source(), None, "a stale index");

        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.buffer = None;
        assert_eq!(editor.active_source(), None, "a vanished note");
        editor.caret = Caret { anchor: 0, head: 3 };
        assert_eq!(
            editor.selected_text(),
            None,
            "a selection over a vanished note"
        );
    }

    #[test]
    fn deletion_takes_the_cluster_the_word_or_the_selection() {
        // the block's own trailing "\n" is a separator, never content, so
        // no deletion here can ever remove it
        // (adr/2026-08-per-line-block-segmentation.md)
        let (_dir, mut editor) = open_note("l'idée\n");
        editor.activate(0);
        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l'idé\n", "the trailing e went, not the separator");

        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l'id\n", "é went whole, not one byte");

        editor.delete_at_caret(Deletion::WordBack);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l'\n", "the word went, the apostrophe stayed");

        editor.move_caret(caret::Move::LineStart, false);
        editor.delete_at_caret(Deletion::Forward);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "'\n");

        editor.move_caret(caret::Move::LineEnd, true);
        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "\n", "the content went whole; the separator stayed");
    }

    #[test]
    fn deletion_at_the_blocks_edges_is_inert() {
        // blocks join by being emptied, never by backspacing across the
        // hidden separator
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.move_caret(caret::Move::LineStart, false);
        editor.delete_at_caret(Deletion::Back);
        editor.delete_at_caret(Deletion::WordBack);
        editor.move_caret(caret::Move::LineEnd, false);
        editor.delete_at_caret(Deletion::Forward);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, NOTE, "nothing to remove, nothing removed");
        assert_eq!(editor.trouble(), None, "and nothing to complain about");
    }

    #[test]
    fn deletion_against_a_stale_editor_is_dropped_loudly() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.delete_at_caret(Deletion::Back);
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // a stale caret refuses quietly: there is no span to remove
        let (_dir, mut editor) = open_note("été\n");
        editor.activate(0);
        editor.caret = Caret { anchor: 1, head: 3 };
        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "été\n", "a refused deletion changes nothing");
    }

    #[test]
    fn horizontal_moves_walk_clusters_and_collapse_selections() {
        let (_dir, mut editor) = open_note("été\n");
        editor.activate(0);
        // from the end (content excludes the trailing separator): left
        // over the second é, then the first
        editor.move_caret(caret::Move::Left, false);
        assert_eq!(editor.caret_in_block().1, 3, "é is one step");
        editor.move_caret(caret::Move::Left, false);
        assert_eq!(editor.caret_in_block().1, 2);

        // shift-left selects; a plain arrow collapses to the matching edge
        editor.move_caret(caret::Move::Left, true);
        assert_eq!(editor.selected_text().as_deref(), Some("é"));
        editor.move_caret(caret::Move::Left, false);
        assert_eq!(editor.selection(), None);
        assert_eq!(editor.caret_in_block().1, 0, "collapsed left");

        editor.move_caret(caret::Move::Right, true);
        editor.move_caret(caret::Move::Right, true);
        assert_eq!(editor.selected_text().as_deref(), Some("ét"));
        editor.move_caret(caret::Move::Right, false);
        assert_eq!(editor.caret_in_block().1, 3, "collapsed right");
        editor.move_caret(caret::Move::Right, false);
        assert_eq!(editor.caret_in_block().1, 5, "stepped over é");
    }

    #[test]
    fn word_moves_and_line_edges_answer() {
        let (_dir, mut editor) = open_note("l'idée est là\n");
        editor.activate(0);
        // the caret opens on the empty last line; up reaches the text
        editor.move_caret(caret::Move::Up, false);
        editor.move_caret(caret::Move::LineStart, false);
        editor.move_caret(caret::Move::WordRight, false);
        assert_eq!(editor.caret_in_block().1, 1, "the end of l");
        editor.move_caret(caret::Move::WordRight, false);
        assert_eq!(editor.caret_in_block().1, 7, "the end of idée");
        editor.move_caret(caret::Move::WordLeft, false);
        assert_eq!(editor.caret_in_block().1, 2, "the start of idée");
        editor.move_caret(caret::Move::LineEnd, false);
        assert_eq!(editor.caret_in_block().1, 15, "before the newline");
    }

    #[test]
    fn vertical_moves_keep_the_goal_column_through_short_lines() {
        // per-line blocks make multi-line vertical movement within one
        // active block rare in practice; a raw fence is the one construct
        // the parser never splits, so it is still where several lines
        // share a block (adr/2026-08-per-line-block-segmentation.md)
        let (_dir, mut editor) =
            open_note("```\npremier\nab\ntroisième\n```\n");
        editor.activate(0);
        // from the closing fence, up to "premier"'s line, its start, then
        // 6 rights to column 6, then down twice: the short "ab" clamps,
        // "troisième" restores the column
        for _ in 0..3 {
            editor.move_caret(caret::Move::Up, false);
        }
        editor.move_caret(caret::Move::LineStart, false);
        for _ in 0..6 {
            editor.move_caret(caret::Move::Right, false);
        }
        editor.move_caret(caret::Move::Down, false);
        assert_eq!(editor.caret_in_block().1, 14, "clamped to ab's end");
        editor.move_caret(caret::Move::Down, false);
        let head = editor.caret_in_block().1;
        let content = "```\npremier\nab\ntroisième\n```";
        assert_eq!(&content[head..head + 2], "è");
        // a horizontal move forgets the goal
        editor.move_caret(caret::Move::Left, false);
        editor.move_caret(caret::Move::Down, false);
        assert_eq!(editor.caret_in_block().1, 29, "clamped to the last line");
    }

    #[test]
    fn vertical_moves_slide_blocks_at_their_edges() {
        // NOTE's blocks: 0 preamble, 1 blank, 2 "= title", 3 blank,
        // 4 "prose", 5 the trailing empty line — sliding from the blank
        // line at 1 still lands on its neighbours at 0 and 2
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.move_caret(caret::Move::Up, false);
        assert_eq!(editor.active(), Some(0), "up from the first line");
        let end = editor.blocks()[0].content().len();
        assert_eq!(editor.caret_in_block(), (end, end), "landed at its end");

        editor.activate(editor.blocks()[1].range.start);
        editor.move_caret(caret::Move::Down, false);
        assert_eq!(editor.active(), Some(2), "down from the last line");
    }

    #[test]
    fn vertical_moves_clamp_at_the_notes_ends() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        // the preamble has three lines; walking past the top clamps
        for _ in 0..3 {
            editor.move_caret(caret::Move::Up, false);
        }
        assert_eq!(editor.active(), Some(0), "no block above the first");
        assert_eq!(editor.caret_in_block().1, 0, "clamped to the start");

        let last = editor.blocks().len() - 1;
        editor.activate(editor.blocks()[last].range.start);
        editor.move_caret(caret::Move::Down, false);
        assert_eq!(editor.active(), Some(last), "no block below the last");
        let end = editor.blocks()[last].content().len();
        assert_eq!(editor.caret_in_block().1, end, "clamped to the end");
    }

    #[test]
    fn a_selecting_vertical_move_stays_inside_the_block() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        editor.move_caret(caret::Move::Up, true);
        assert_eq!(editor.active(), Some(2), "no slide while selecting");
        assert_eq!(editor.caret_in_block().1, 0, "clamped to the start");
        assert_eq!(editor.selected_text().as_deref(), Some("= title"));

        editor.move_caret(caret::Move::Down, true);
        assert_eq!(editor.active(), Some(2));
        assert_eq!(editor.selection(), None, "back to the anchor");
    }

    #[test]
    fn moves_with_nothing_active_are_inert() {
        let mut editor = Editor::closed();
        editor.move_caret(caret::Move::Up, false);
        editor.place_in_block(0, 0, false);
        editor.select_all();
        editor.slide(true);
        assert_eq!(editor.active(), None);
        assert_eq!(editor.trouble(), None);

        // a slide with no neighbour in that direction holds
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.slide(true);
        assert_eq!(editor.active(), Some(0), "no block above the first");

        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.move_caret(caret::Move::Up, false);
        assert_eq!(editor.active(), None, "rendered view: arrows are inert");
    }

    #[test]
    fn select_all_takes_the_block_not_the_note() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        editor.select_all();
        assert_eq!(editor.selected_text().as_deref(), Some("= title"));
    }

    #[test]
    fn place_in_block_converts_the_probes_utf16_answer() {
        let (_dir, mut editor) = open_note("été\n\nprose\n");
        editor.activate(0);
        // the probe answers (span start, UTF-16 units within it): "été" is
        // five bytes but three units — after the first é is byte 2
        editor.place_in_block(0, 1, false);
        assert_eq!(editor.caret_in_block(), (2, 2));
        // a drag extends from the anchor
        editor.place_in_block(0, 3, true);
        assert_eq!(editor.selected_text().as_deref(), Some("té"));
        // a probe miss lands at the block's end
        editor.place_in_block(usize::MAX, 0, false);
        assert_eq!(editor.caret_in_block().1, 5, "the end of été");
    }

    #[test]
    fn splice_within_the_block_rides_the_typing_path() {
        // NOTE's blocks: 0 preamble, 1 blank, 2 "= title", 3 blank,
        // 4 "prose", 5 the trailing empty line
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        let content = editor.blocks()[2].content();

        // replace "title" with "titre": no resegment, later blocks shift
        let start = content.start + 2;
        editor.splice(start..start + 5, "titre", start);
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= titre"), "{text}");
        assert!(text.ends_with("prose\n"), "later blocks survive: {text}");
        assert_eq!(editor.active(), Some(2), "the block held");
        assert_eq!(editor.caret().map(|caret| caret.head), Some(start));
    }

    #[test]
    fn splice_across_blocks_resegments_and_wakes_the_carets_block() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);

        // delete from the title through the whole prose block
        let start = editor.blocks()[1].range.start;
        editor.splice(start..NOTE.len(), "", start);
        let (_, text) = editor.note().expect("still open");
        assert!(!text.contains("title"), "{text}");
        assert!(!text.contains("prose"), "{text}");
        let woken = editor.active().expect("a block woke");
        assert_eq!(
            editor.blocks().len() - 1,
            woken,
            "the caret's block is the last one now"
        );
    }

    #[test]
    fn splice_guards_stay_loud() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.splice(0..1, "x", 0);
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // a span off a char boundary, inside the block
        let (_dir, mut editor) = open_note("été\n");
        editor.activate(0);
        editor.splice(1..3, "x", 0);
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // and across: a reversed span the buffer refuses
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.splice(NOTE.len()..0, "x", 0);
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, NOTE, "a refused splice changes nothing");
    }

    #[test]
    fn splice_survives_a_block_map_that_outgrew_its_note() {
        // within-shaped span over a content_end past the buffer
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks[0].content_end = NOTE.len() + 40;
        editor.splice(2..4, "x", 2);
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));

        // across-shaped span with the buffer gone from under the blocks
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.buffer = None;
        editor.splice(0..NOTE.len(), "", 0);
        assert_eq!(editor.trouble(), Some(&Trouble::Stale));
    }

    #[test]
    fn dd_on_the_only_line_never_leaves_the_editor_without_an_active_block() {
        // blocks::segment("") always returns one block, so dd emptying the
        // note's only line can never leave the editor without one to
        // activate (adr/2026-08-editor-splice-cross-block.md)
        let (_dir, mut editor) = open_note("seule");
        assert_eq!(editor.blocks().len(), 1);

        let mut vim = crate::vim::Vim::default();
        let mut outcome = crate::vim::Outcome::Swallow;
        for _ in 0..2 {
            let caret = editor.caret().expect("a caret is active");
            let (_, text) = editor.note().expect("the note is open");
            outcome = vim.handle(
                &dioxus::html::Key::Character("d".to_string()),
                dioxus::html::Modifiers::empty(),
                &crate::vim::View {
                    text,
                    blocks: editor.blocks(),
                    head: caret.head,
                    anchor: caret.anchor,
                },
            );
        }
        let crate::vim::Outcome::Acts(acts) = outcome else {
            panic!("dd should emit acts, got {outcome:?}");
        };
        for act in acts {
            match act {
                crate::vim::Act::Checkpoint => editor.checkpoint(),
                crate::vim::Act::Splice { span, text, caret } => {
                    editor.splice(span, &text, caret);
                }
                crate::vim::Act::Place(at) => editor.place_at(at),
                crate::vim::Act::SetClipboard(_) => {}
                other => panic!("unexpected act for dd: {other:?}"),
            }
        }

        assert_eq!(editor.blocks().len(), 1);
        assert_eq!(editor.active(), Some(0));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "");
        assert_eq!(editor.caret().map(|caret| caret.head), Some(0));
    }

    #[test]
    fn paste_lands_the_clip_and_declines_when_closed() {
        let (_dir, mut editor) = open_note("un mot\n");
        editor.activate(0);
        editor.place_at(3);
        editor.paste("beau ", true, 1);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "un beau mot\n");

        // a paste with nothing under it is inert, twice over
        let mut closed = Editor::closed();
        closed.paste("x", false, 1);
        assert_eq!(closed.trouble(), None);
        let (_dir, mut editor) = open_note("un mot\n");
        editor.deactivate();
        editor.paste("x", false, 1);
        assert_eq!(editor.caret(), None);

        // and a note vanished from under its blocks declines too
        let (_dir, mut editor) = open_note("un mot\n");
        editor.activate(0);
        editor.buffer = None;
        editor.paste("x", false, 1);
        assert_eq!(editor.note(), None);
    }

    #[test]
    fn paste_folds_the_no_break_spaces_the_clip_carries() {
        // the fold guards vim's p as well as the typing path
        // (adr/2026-08-nbsp-folded-on-buffer-entry.md)
        let (_dir, mut editor) = open_note("un mot\n");
        editor.activate(0);
        editor.place_at(3);
        editor.paste("beau\u{a0}", true, 1);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "un beau mot\n");
    }

    #[test]
    fn place_at_wakes_the_block_that_owns_the_offset() {
        // NOTE's blocks: 0 preamble, 1 blank, 2 "= title", 3 blank,
        // 4 "prose", 5 the trailing empty line
        let (_dir, mut editor) = open_note(NOTE);
        assert_eq!(editor.active(), Some(5));

        // a landing outside the active block wakes its owner
        let target = editor.blocks()[0].content().start + 2;
        editor.place_at(target);
        assert_eq!(editor.active(), Some(0));
        assert_eq!(editor.caret().map(|caret| caret.head), Some(target));

        // a landing inside it just moves the caret
        editor.place_at(target + 3);
        assert_eq!(editor.active(), Some(0));
        assert_eq!(editor.caret().map(|caret| caret.head), Some(target + 3));

        // a stale coordinate floors to a boundary; past the end clamps
        let (_dir, mut editor) = open_note("été\n");
        editor.place_at(1);
        assert_eq!(editor.caret().map(|caret| caret.head), Some(0));
        editor.place_at(99);
        assert_eq!(editor.caret().map(|caret| caret.head), Some(6));

        // nothing open, nothing to place
        let mut editor = Editor::closed();
        editor.place_at(3);
        assert_eq!(editor.caret(), None);
    }

    #[test]
    fn extend_and_swap_keep_the_anchor_across_blocks() {
        // NOTE's blocks: 0 preamble, 1 blank, 2 "= title", 3 blank,
        // 4 "prose", 5 the trailing empty line
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        let start = editor.blocks()[2].content().start;
        editor.place_at(start);

        // extending into another block wakes it, the anchor holding
        editor.extend_to(NOTE.len() - 2);
        assert_eq!(editor.active(), Some(4), "the prose block woke");
        let caret = editor.caret().expect("a caret");
        assert_eq!(caret.anchor, start, "the far end held");
        assert_eq!(caret.head, NOTE.len() - 2);

        // o jumps back to the far end, waking its block again
        editor.swap_ends();
        assert_eq!(editor.active(), Some(2));
        let caret = editor.caret().expect("a caret");
        assert_eq!(caret.head, start);
        assert_eq!(caret.anchor, NOTE.len() - 2);

        // closed editors absorb both
        let mut closed = Editor::closed();
        closed.extend_to(5);
        closed.swap_ends();
        assert_eq!(closed.caret(), None);
    }

    #[test]
    fn a_closed_editor_absorbs_activation_and_deactivation() {
        let mut editor = Editor::closed();
        editor.activate(5);
        assert_eq!(editor.active(), None, "nothing to activate");
        editor.deactivate();
        assert_eq!(editor.trouble(), None, "nothing to save");
    }

    #[test]
    fn undo_walks_change_intents_and_redo_returns() {
        let (_dir, mut editor) = open_note("un\n");
        editor.activate(0);
        // intent one: type at the end
        editor.checkpoint();
        editor.place_at(2);
        editor.insert_at_caret(" mot");
        // intent two: another burst
        editor.checkpoint();
        editor.insert_at_caret(" bleu");
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un mot bleu\n");

        editor.undo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un mot\n", "one intent, one step");
        editor.undo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un\n", "back to the opened file");
        editor.undo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un\n", "the bottom holds");

        editor.redo();
        editor.redo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un mot bleu\n");
        editor.redo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un mot bleu\n", "the top holds");

        // a new intent clears the redo path
        editor.undo();
        editor.checkpoint();
        editor.insert_at_caret("!");
        editor.redo();
        let (_, text) = editor.note().expect("open");
        assert!(text.contains('!'), "redo found nothing to redo: {text}");
    }

    #[test]
    fn abandoned_checkpoints_cost_no_press() {
        let (_dir, mut editor) = open_note("un\n");
        editor.activate(0);
        editor.insert_at_caret("x");
        // three entered-then-abandoned sessions checkpoint the same state
        editor.checkpoint();
        editor.checkpoint();
        editor.checkpoint();
        editor.undo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "un\n", "one press, straight past the noise");
    }

    #[test]
    fn undo_restores_across_blocks_and_survives_the_edges() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[0].range.start);
        editor.checkpoint();
        editor.splice(0..NOTE.len(), "", 0);
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, "");

        editor.undo();
        let (_, text) = editor.note().expect("open");
        assert_eq!(text, NOTE, "the whole note came back");
        assert!(editor.active().is_some(), "with a block awake");

        // closed editors absorb everything
        let mut closed = Editor::closed();
        closed.checkpoint();
        closed.undo();
        closed.redo();
        assert_eq!(closed.note(), None);

        // a redo whose note vanished mid-flight declines
        let (_dir, mut editor) = open_note("un\n");
        editor.activate(0);
        editor.checkpoint();
        editor.insert_at_caret("x");
        editor.undo();
        editor.buffer = None;
        editor.redo();
        assert_eq!(editor.note(), None);
    }

    #[test]
    fn the_history_depth_is_bounded() {
        let (_dir, mut editor) = open_note("0\n");
        editor.activate(0);
        for step in 1..=120u32 {
            editor.checkpoint();
            editor.select_all();
            editor.insert_at_caret(&format!("{step}\n"));
        }
        for _ in 0..200 {
            editor.undo();
        }
        let (_, text) = editor.note().expect("open");
        assert_eq!(
            text, "20\n\n",
            "the oldest steps fell off a hundred deep; the note's own \
             separator from the opened file is untouched by content edits"
        );
    }

    #[test]
    fn typing_round_trips_byte_identical() {
        // the roadmap's phase-0 exit test: a French corpus driven one
        // keystroke at a time arrives on disk byte-identical
        let corpus = "L'été à Montréal — cœur, naïveté, ça va.\n";
        let (dir, mut editor) = open_note("");
        editor.activate(0);
        for cluster in
            unicode_segmentation::UnicodeSegmentation::graphemes(corpus, true)
        {
            if cluster == "\n" {
                editor.insert_at_caret("\n");
            } else {
                editor.insert_at_caret(cluster);
            }
        }
        editor.deactivate();
        let saved = std::fs::read_to_string(dir.path().join("note.typ"))
            .expect("the note is readable");
        assert_eq!(saved, corpus);
        assert_eq!(editor.trouble(), None);
    }

    #[test]
    fn flush_reports_the_save_and_deposits_only_failures() {
        // nothing open: trivially flushed, and a pending deposit — the open
        // failure here — is not this flush's to clear
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let mut editor = Editor::open(dir.path().join("absente.typ"));
        assert!(editor.flush(), "nothing open, nothing to lose");
        assert!(
            matches!(editor.trouble(), Some(Trouble::Open(_))),
            "the undrained trouble survives the flush: {:?}",
            editor.trouble()
        );

        // open and writable: flushed, nothing deposited
        let (dir, mut editor) = open_note(NOTE);
        assert!(editor.flush());
        assert_eq!(editor.trouble(), None);

        // open in a locked directory: the failure cancels and is deposited
        lock(dir.path(), true);
        assert!(!editor.flush(), "a failed save must cancel a quit");
        lock(dir.path(), false);
        match editor.take_trouble() {
            Some(Trouble::Save(detail)) => {
                assert!(detail.contains("note.typ"), "{detail}");
            }
            other => panic!("the failure is deposited: {other:?}"),
        }
    }

    #[test]
    fn save_reports_without_touching_the_editor() {
        assert_eq!(Editor::closed().save(), None, "nothing open");

        let (dir, editor) = open_note(NOTE);
        assert_eq!(editor.save(), None, "a writable note saves");

        lock(dir.path(), true);
        let trouble = editor.save().expect("the failure is returned");
        lock(dir.path(), false);
        match trouble {
            Trouble::Save(detail) => {
                assert!(detail.contains("note.typ"), "{detail}");
            }
            other => panic!("a refused disk is a save failure: {other:?}"),
        }
        assert_eq!(editor.trouble(), None, "save never deposits");
    }

    #[test]
    fn save_reports_a_file_it_is_not_allowed_to_write() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "= read only\n").expect("the note is written");

        let buffer = Buffer::open(file.clone()).expect("the note opens");
        lock(dir.path(), true);
        let error = buffer.save().unwrap_err();
        lock(dir.path(), false);
        match error {
            SaveError::Io(error) => {
                assert_eq!(error.kind(), ErrorKind::PermissionDenied);
            }
            other => panic!("a refused disk is io, not conflict: {other:?}"),
        }
    }

    #[test]
    fn an_external_edit_refuses_the_save_and_both_versions_survive() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.edit("= mine\n\n");

        let file = dir.path().join("note.typ");
        edit_behind(&file, "= theirs\n");

        assert_eq!(editor.save(), Some(Trouble::Conflict));
        assert!(
            !editor.flush(),
            "a conflict cancels a quit like any refusal"
        );
        assert_eq!(editor.take_trouble(), Some(Trouble::Conflict));
        assert_eq!(
            std::fs::read_to_string(&file).expect("the note is readable"),
            "= theirs\n",
            "the outside author was not clobbered"
        );
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= mine"), "the buffer kept its side: {text}");
    }

    #[test]
    fn clobber_takes_the_buffers_side_and_rearms_the_guard() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.edit("= mine\n\n");

        let file = dir.path().join("note.typ");
        edit_behind(&file, "= theirs\n");

        assert!(editor.clobber(), "keep-mine lands");
        let saved =
            std::fs::read_to_string(&file).expect("the note is readable");
        assert!(saved.contains("= mine"), "{saved}");
        assert_eq!(editor.save(), None, "the guard accepts its own write");
        assert_eq!(editor.trouble(), None);
    }

    #[test]
    fn clobber_reports_a_disk_that_refuses_and_a_closed_editor_is_a_no_op() {
        assert!(Editor::closed().clobber(), "nothing open, nothing to fail");

        let (dir, mut editor) = open_note(NOTE);
        lock(dir.path(), true);
        assert!(!editor.clobber());
        lock(dir.path(), false);
        match editor.take_trouble() {
            Some(Trouble::Save(detail)) => {
                assert!(detail.contains("note.typ"), "{detail}");
            }
            other => panic!("the failure is deposited: {other:?}"),
        }
    }

    #[test]
    fn a_vanished_file_is_recreated_not_a_conflict() {
        // deleting is not authorship: rewriting the user's own text over a
        // hole erases no one
        let (dir, editor) = open_note(NOTE);
        let file = dir.path().join("note.typ");
        std::fs::remove_file(&file).expect("the note is deleted outside");

        assert_eq!(editor.save(), None, "the save recreates the file");
        assert!(file.exists());
        assert_eq!(editor.save(), None, "and the guard re-armed on it");
    }
}
