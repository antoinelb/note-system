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
    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }
    pub fn save(&self) -> Result<(), std::io::Error> {
        std::fs::write(&self.file, &self.text)
    }
}

/// Applies an edit to whatever note is open. An edit with nothing open is
/// dropped: the editor widget only exists while a buffer does, so this makes
/// the unrepresentable case explicit instead of unwrapping it.
pub fn apply_edit(open: &mut Option<Buffer>, text: String) {
    if let Some(note) = open {
        note.set_text(text);
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
    fn set_text_replaces_the_whole_text_and_save_round_trips_it() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "before\n").expect("the note is written");

        let mut buffer = Buffer::open(file.clone()).expect("the note opens");
        buffer.set_text("after\n".to_string());
        assert_eq!(buffer.text(), "after\n");

        // nothing reaches disk until save is called
        assert_eq!(
            std::fs::read_to_string(&file).expect("the note is readable"),
            "before\n"
        );

        buffer.save().expect("the note saves");
        let reopened = Buffer::open(file).expect("the note reopens");
        assert_eq!(reopened.text(), "after\n");
    }

    #[test]
    fn an_edit_reaches_the_open_note() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let file = dir.path().join("note.typ");
        std::fs::write(&file, "before\n").expect("the note is written");

        let mut open = Some(Buffer::open(file).expect("the note opens"));
        apply_edit(&mut open, "after\n".to_string());
        assert_eq!(
            open.as_ref().map(Buffer::text),
            Some("after\n"),
            "the edit did not reach the buffer"
        );
    }

    #[test]
    fn an_edit_with_no_note_open_is_dropped() {
        let mut open = None;
        apply_edit(&mut open, "nowhere to go".to_string());
        assert!(open.is_none(), "an orphaned edit invented a buffer");
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
