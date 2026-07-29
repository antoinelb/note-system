use std::ops::Range;
use std::path::{Path, PathBuf};

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
