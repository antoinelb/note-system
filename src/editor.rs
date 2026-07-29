use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::blocks::{self, Block};

/// The edit-command layer over one open note: the buffer, its block map,
/// the active block and the notice the widget should surface. The widget
/// only forwards events here — the v2 modal keymap slots in between the two
/// without touching either (plan.md § Editor,
/// adr/2026-07-hybrid-active-block-textarea.md).
#[derive(Debug, Default)]
pub struct Editor {
    buffer: Option<Buffer>,
    blocks: Vec<Block>,
    active: Option<usize>,
    notice: Option<String>,
}

/// Every guard in `edit` failing means the widget diverged from the buffer
/// — a bug, surfaced as a visible notice rather than silently eaten input.
const STALE_EDIT: &str = "edit dropped: the editor lost its block";

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
            Ok(note) => Editor {
                blocks: blocks::segment(note.text()),
                buffer: Some(note),
                ..Editor::default()
            },
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

    /// One widget keystroke: the active block's whole new text, spliced into
    /// the buffer, with every later block shifted by the delta. No reparse —
    /// block boundaries move only on activate/deactivate.
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
        if note.replace_range(block.range.clone(), value) {
            blocks::resize(&mut self.blocks, index, value.len());
        } else {
            self.notice = Some(STALE_EDIT.to_string());
        }
    }

    /// A click on a rendered block: flush any pending edit, resegment (the
    /// edit may have split or merged blocks), then land on the block owning
    /// the clicked block's first byte — a coordinate, so it survives the
    /// index shuffle resegmentation can cause.
    pub fn activate(&mut self, start: usize) {
        self.deactivate();
        self.active = (!self.blocks.is_empty())
            .then(|| blocks::block_at(&self.blocks, start));
    }

    /// Escape or clicking away: save, resegment, back to fully rendered. A
    /// failed save is the notice — the text survives in the buffer.
    pub fn deactivate(&mut self) {
        self.notice = self.buffer.as_ref().and_then(|note| {
            note.save()
                .err()
                .map(|err| format!("{}: {err}", note.file().display()))
        });
        self.blocks = self
            .buffer
            .as_ref()
            .map(|note| blocks::segment(note.text()))
            .unwrap_or_default();
        self.active = None;
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
    fn open_segments_the_note_with_nothing_active() {
        let (_dir, editor) = open_note(NOTE);
        assert_eq!(editor.blocks().len(), 3, "{:?}", editor.blocks());
        assert_eq!(editor.active(), None);
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
        // no block active
        let (_dir, mut editor) = open_note(NOTE);
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
        editor.blocks[0].range.end = NOTE.len() + 40;
        editor.edit("anything");
        assert_eq!(editor.notice(), Some(STALE_EDIT));
        let (_, text) = editor.note().expect("still open");
        assert_eq!(text, NOTE, "a refused edit changes nothing");
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
