use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::blocks::{self, Block};
use crate::caret;

/// The edit-command layer over one open note: the buffer, its block map,
/// the active block, the caret and the notice the widget should surface.
/// The widget only forwards events here — the v2 modal keymap slots in
/// between the two without touching either (plan.md § Editor,
/// adr/2026-07-hybrid-active-block-textarea.md).
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
    notice: Option<String>,
}

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

/// Every guard in `edit` failing means the widget diverged from the buffer
/// — a bug, surfaced as a visible notice rather than silently eaten input.
const STALE_EDIT: &str = "edit dropped: the editor lost its block";

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
    /// closed editor carrying the error as its notice.
    pub fn open(file: PathBuf) -> Editor {
        match Buffer::open(file.clone()) {
            Ok(note) => {
                let blocks = blocks::segment(note.text());
                let end = note.text().len();
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
                    ..Editor::default()
                }
            }
            Err(err) => Editor {
                notice: Some(format!("{}: {err}", file.display())),
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

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Errors from outside the editor (note creation) share the notice line.
    pub fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    /// One widget edit: the active block's whole new content, spliced into
    /// the buffer (the trailing separator is not the widget's to touch),
    /// with every later block shifted by the delta. No reparse — block
    /// boundaries move only on activate/deactivate.
    pub fn edit(&mut self, value: &str) {
        let (Some(index), Some(note)) = (self.active, self.buffer.as_mut())
        else {
            self.notice = Some(STALE_EDIT.to_string());
            return;
        };
        let Some(block) = self.blocks.get(index) else {
            self.notice = Some(STALE_EDIT.to_string());
            return;
        };
        if note.replace_range(block.content(), value) {
            blocks::resize(&mut self.blocks, index, value.len());
        } else {
            self.notice = Some(STALE_EDIT.to_string());
        }
    }

    /// Every insertion the widget makes — typing, Enter, paste, a committed
    /// composition, an accepted link completion: `text` replaces the
    /// selection (or lands at the collapsed caret) and the caret ends after
    /// it. Routed through `edit` — the block's whole new content, one
    /// staleness policy (adr/2026-08-ctrl-l-link-picker.md).
    pub fn insert_at_caret(&mut self, text: &str) {
        let Some((content, source)) = self.active_slice() else {
            self.notice = Some(STALE_EDIT.to_string());
            return;
        };
        let (anchor, head) = self.caret_in_block();
        let span = anchor.min(head)..anchor.max(head);
        if !source.is_char_boundary(span.start)
            || !source.is_char_boundary(span.end)
        {
            self.notice = Some(STALE_EDIT.to_string());
            return;
        }
        let mut value = source;
        value.replace_range(span.clone(), text);
        // `active_slice` proved the block fits the buffer, which is the
        // same validity `edit` re-checks — the splice cannot refuse here
        self.edit(&value);
        self.place(content.start + span.start + text.len());
    }

    /// A deletion keystroke: the selection when one exists, otherwise the
    /// cluster or word the key names. At the block's edge with nothing to
    /// remove, nothing happens — blocks join by being emptied, never by
    /// backspacing across the hidden separator.
    pub fn delete_at_caret(&mut self, kind: Deletion) {
        let Some((content, source)) = self.active_slice() else {
            self.notice = Some(STALE_EDIT.to_string());
            return;
        };
        let (anchor, head) = self.caret_in_block();
        let span = if anchor != head {
            anchor.min(head)..anchor.max(head)
        } else {
            match kind {
                Deletion::Back => caret::prev_cluster(&source, head)..head,
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

    /// A motion's landing: the caret collapses to a note-global offset,
    /// waking the block that owns it when it lies outside the active one —
    /// the same flush-and-resegment path as a click, with the coordinate
    /// riding through because resegmenting never edits
    /// (adr/2026-08-caret-on-editor-note-bytes.md). Phase 5's search lands
    /// through this too.
    pub fn place_at(&mut self, offset: usize) {
        let Some((_, text)) = self.note() else { return };
        let offset = floor_boundary(text, offset);
        let inside = self.active_content().is_some_and(|content| {
            offset >= content.start && offset <= content.end
        });
        if !inside {
            self.activate(offset);
        }
        self.place(offset);
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
    /// failed save is the notice — the text survives in the buffer.
    pub fn deactivate(&mut self) {
        self.flush();
        self.blocks = self
            .buffer
            .as_ref()
            .map(|note| blocks::segment(note.text()))
            .unwrap_or_default();
        self.active = None;
    }

    /// Ctrl+Q and deactivation: save and surface the outcome. Returns
    /// whether the note reached disk, so a failed flush can cancel a quit
    /// instead of losing the buffer
    /// (adr/2026-07-ctrl-q-flushes-then-closes.md).
    pub fn flush(&mut self) -> bool {
        if self.buffer.is_none() {
            // nothing open, nothing to lose — and any pending notice (a
            // create error) is not this flush's to clear
            return true;
        }
        self.notice = self.save();
        self.notice.is_none()
    }

    /// The autosave tick: saves the open note without touching editor
    /// state. The caller decides how to surface a failure — the autosave
    /// resource must not write the signal it subscribes to unguarded, or it
    /// would restart itself forever.
    pub fn save(&self) -> Option<String> {
        self.buffer.as_ref().and_then(|note| {
            note.save()
                .err()
                .map(|err| format!("{}: {err}", note.file().display()))
        })
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

#[derive(Debug)]
pub struct Buffer {
    file: PathBuf,
    text: String,
}

impl Buffer {
    pub fn open(file: PathBuf) -> Result<Buffer, std::io::Error> {
        let text = std::fs::read_to_string(&file)?;
        Ok(Buffer { file, text })
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
    pub fn save(&self) -> Result<(), std::io::Error> {
        std::fs::write(&self.file, &self.text)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::io::ErrorKind;

    use super::*;

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
        assert_eq!(editor.blocks().len(), 3, "{:?}", editor.blocks());
        assert_eq!(editor.active(), Some(2));
        assert_eq!(editor.notice(), None);
        let (file, text) = editor.note().expect("the note is open");
        assert!(file.ends_with("note.typ"));
        assert_eq!(text, NOTE);
    }

    #[test]
    fn open_failure_is_a_closed_editor_carrying_the_error() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let editor = Editor::open(dir.path().join("absente.typ"));
        assert!(editor.note().is_none());
        assert!(editor.blocks().is_empty());
        let notice = editor.notice().expect("the failure is the notice");
        assert!(notice.contains("absente.typ"), "{notice}");
    }

    #[test]
    fn closed_is_empty_and_accepts_a_notice() {
        let mut editor = Editor::closed();
        assert!(editor.note().is_none());
        editor.set_notice("create: boom".to_string());
        assert_eq!(editor.notice(), Some("create: boom"));
    }

    #[test]
    fn activate_lands_on_the_clicked_block_and_edit_splices() {
        let (_dir, mut editor) = open_note(NOTE);
        let start = editor.blocks()[1].range.start;
        editor.activate(start);
        assert_eq!(editor.active(), Some(1));

        editor.edit("= new title\n\n");
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= new title"), "{text}");
        assert!(text.ends_with("prose\n"), "later blocks survive: {text}");
        assert_eq!(
            editor.blocks()[2].range.end,
            text.len(),
            "later spans shifted with the edit"
        );
    }

    #[test]
    fn deactivate_saves_resegments_and_clears_the_active_block() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        // a blank line typed inside the block splits it on deactivate
        editor.edit("prose\n\nencore\n");
        editor.deactivate();

        assert_eq!(editor.active(), None);
        assert_eq!(editor.notice(), None);
        assert_eq!(editor.blocks().len(), 4, "{:?}", editor.blocks());
        let saved = std::fs::read_to_string(dir.path().join("note.typ"))
            .expect("the note is readable");
        assert!(saved.contains("encore"), "{saved}");
    }

    #[test]
    fn activate_flushes_the_previous_block_before_moving() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.edit("= renamed\n\n");
        editor.activate(editor.blocks()[2].range.start);

        assert_eq!(editor.active(), Some(2));
        let saved = std::fs::read_to_string(dir.path().join("note.typ"))
            .expect("the note is readable");
        assert!(saved.contains("= renamed"), "the move saved: {saved}");
    }

    #[test]
    fn a_failed_save_becomes_the_notice_and_the_text_survives() {
        let (dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.edit("= unsaved\n\n");

        let file = dir.path().join("note.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        editor.deactivate();
        let notice = editor.notice().expect("the save failure is visible");
        assert!(notice.contains("note.typ"), "{notice}");
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= unsaved"), "nothing was lost: {text}");
    }

    #[test]
    fn edits_against_a_stale_editor_are_dropped_loudly() {
        // no block active: only deactivation reaches it now
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.edit("anything");
        assert_eq!(editor.notice(), Some(STALE_EDIT));

        // an active index the block map no longer has
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks.clear();
        editor.edit("anything");
        assert_eq!(editor.notice(), Some(STALE_EDIT));

        // a span the buffer refuses
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks[0].content_end = NOTE.len() + 40;
        editor.edit("anything");
        assert_eq!(editor.notice(), Some(STALE_EDIT));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, NOTE, "a refused edit changes nothing");
    }

    #[test]
    fn emptying_a_blocks_content_merges_it_away() {
        // the separator is not the widget's to touch, so joining paragraphs
        // works by emptying one: the bare separator left behind is absorbed
        // at the next resegmentation
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        editor.edit("");
        editor.deactivate();
        assert_eq!(editor.blocks().len(), 2, "{:?}", editor.blocks());
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
        assert_eq!(editor.caret_in_block(), (6, 6), "prose\\n is six bytes");
    }

    #[test]
    fn activation_lands_the_caret_at_the_woken_blocks_end() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[1].range.start);
        let content = editor.blocks()[1].content();
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
        editor.activate(editor.blocks()[1].range.start);
        editor.move_caret(caret::Move::LineStart, false);
        editor.move_caret(caret::Move::Right, false);
        editor.move_caret(caret::Move::Right, false);
        editor.insert_at_caret("é");
        let (_, text) = editor.note().expect("still open");
        assert!(text.contains("= étitle"), "{text}");
        assert!(text.ends_with("prose\n"), "later blocks survive: {text}");
        assert_eq!(editor.caret_in_block(), (4, 4), "after the é");
        assert_eq!(editor.notice(), None);
    }

    #[test]
    fn insert_at_caret_replaces_the_selection() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(editor.blocks()[2].range.start);
        // the caret opens on the empty last line; up reaches the prose
        editor.move_caret(caret::Move::Up, false);
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
    fn insert_at_caret_against_a_stale_editor_is_dropped_loudly() {
        // no block active: only deactivation reaches it now
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.insert_at_caret("x");
        assert_eq!(editor.notice(), Some(STALE_EDIT));

        // an active index the block map no longer has
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks.clear();
        editor.insert_at_caret("x");
        assert_eq!(editor.notice(), Some(STALE_EDIT));

        // a caret off a char boundary is a stale coordinate
        let (_dir, mut editor) = open_note("été\n");
        editor.activate(0);
        editor.caret = Caret { anchor: 1, head: 1 };
        editor.insert_at_caret("x");
        assert_eq!(editor.notice(), Some(STALE_EDIT));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "été\n", "a refused insert changes nothing");

        // a block whose span no longer fits the buffer
        let (_dir, mut editor) = open_note(NOTE);
        editor.activate(0);
        editor.blocks[0].content_end = NOTE.len() + 40;
        editor.insert_at_caret("x");
        assert_eq!(editor.notice(), Some(STALE_EDIT));
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
        let (_dir, mut editor) = open_note("l'idée\n");
        editor.activate(0);
        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l'idée", "the trailing newline went");

        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l'idé", "é went whole, not one byte");

        editor.delete_at_caret(Deletion::WordBack);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "l'", "the word went, the apostrophe stayed");

        editor.move_caret(caret::Move::LineStart, false);
        editor.delete_at_caret(Deletion::Forward);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "'");

        editor.move_caret(caret::Move::LineEnd, true);
        editor.delete_at_caret(Deletion::Back);
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, "", "the selection went whole");
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
        assert_eq!(editor.notice(), None, "and nothing to complain about");
    }

    #[test]
    fn deletion_against_a_stale_editor_is_dropped_loudly() {
        let (_dir, mut editor) = open_note(NOTE);
        editor.deactivate();
        editor.delete_at_caret(Deletion::Back);
        assert_eq!(editor.notice(), Some(STALE_EDIT));

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
        // from the end: left over the newline, then over the second é
        editor.move_caret(caret::Move::Left, false);
        assert_eq!(editor.caret_in_block().1, 5);
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
        let (_dir, mut editor) = open_note("premier\nab\ntroisième\n");
        editor.activate(0);
        // to line 0's column 6, then down twice: the short line clamps,
        // the long line restores the column
        for _ in 0..3 {
            editor.move_caret(caret::Move::Up, false);
        }
        for _ in 0..6 {
            editor.move_caret(caret::Move::Right, false);
        }
        editor.move_caret(caret::Move::Down, false);
        assert_eq!(editor.caret_in_block().1, 10, "clamped to ab's end");
        editor.move_caret(caret::Move::Down, false);
        let head = editor.caret_in_block().1;
        assert_eq!(&"premier\nab\ntroisième\n"[head..head + 2], "è");
        // a horizontal move forgets the goal
        editor.move_caret(caret::Move::Left, false);
        editor.move_caret(caret::Move::Down, false);
        assert_eq!(editor.caret_in_block().1, 22, "clamped to the last line");
    }

    #[test]
    fn vertical_moves_slide_blocks_at_their_edges() {
        // NOTE's blocks: 0 preamble, 1 "= title\n\n", 2 "prose\n"
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
        editor.activate(editor.blocks()[1].range.start);
        editor.move_caret(caret::Move::Up, true);
        assert_eq!(editor.active(), Some(1), "no slide while selecting");
        assert_eq!(editor.caret_in_block().1, 0, "clamped to the start");
        assert_eq!(editor.selected_text().as_deref(), Some("= title"));

        editor.move_caret(caret::Move::Down, true);
        assert_eq!(editor.active(), Some(1));
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
        assert_eq!(editor.notice(), None);

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
        editor.activate(editor.blocks()[1].range.start);
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
    fn place_at_wakes_the_block_that_owns_the_offset() {
        // NOTE's blocks: 0 preamble, 1 "= title\n\n", 2 "prose\n"
        let (_dir, mut editor) = open_note(NOTE);
        assert_eq!(editor.active(), Some(2));

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
    fn a_closed_editor_absorbs_activation_and_deactivation() {
        let mut editor = Editor::closed();
        editor.activate(5);
        assert_eq!(editor.active(), None, "nothing to activate");
        editor.deactivate();
        assert_eq!(editor.notice(), None, "nothing to save");
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
        assert_eq!(editor.notice(), None);
    }

    #[test]
    fn flush_reports_the_save_and_owns_only_save_notices() {
        // nothing open: trivially flushed, and a create error survives
        let mut editor = Editor::closed();
        editor.set_notice("create: boom".to_string());
        assert!(editor.flush(), "nothing open, nothing to lose");
        assert_eq!(editor.notice(), Some("create: boom"));

        // open and writable: flushed, and a stale notice is cleared
        let (dir, mut editor) = open_note(NOTE);
        editor.set_notice("stale".to_string());
        assert!(editor.flush());
        assert_eq!(editor.notice(), None);

        // open and read-only: the failure cancels and is the notice
        let file = dir.path().join("note.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");
        assert!(!editor.flush(), "a failed save must cancel a quit");
        let notice = editor.notice().expect("the failure is visible");
        assert!(notice.contains("note.typ"), "{notice}");
    }

    #[test]
    fn save_reports_without_touching_the_editor() {
        assert_eq!(Editor::closed().save(), None, "nothing open");

        let (dir, editor) = open_note(NOTE);
        assert_eq!(editor.save(), None, "a writable note saves");

        let file = dir.path().join("note.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");
        let error = editor.save().expect("the failure is returned");
        assert!(error.contains("note.typ"), "{error}");
        assert_eq!(editor.notice(), None, "save never writes the notice");
    }

    #[test]
    fn save_reports_a_file_it_is_not_allowed_to_write() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "= read only\n").expect("the note is written");

        let buffer = Buffer::open(file.clone()).expect("the note opens");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        let error = buffer.save().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{error}");
    }
}
