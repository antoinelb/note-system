//! Caret memory: vault-relative path → the line and column the caret was
//! left on, in a plain-lines file under `.index/`, beside the positions and
//! the usage counts and never inside the database
//! (adr/2026-09-a-note-reopens-where-it-was-left.md).
//!
//! Nothing derives these places — they are the record of where the user
//! stopped reading, the same class of user data as a canvas position
//! (adr/2026-07-positions-separate-file.md). A missing or unreadable file
//! is "nothing remembered", never an error: every note then opens where a
//! note nobody has opened opens, at the end of its title heading.
//!
//! The value is a line and a column, not a byte offset, because the file
//! may have been edited outside the app between the two visits: a line
//! number clamps to the note's last line and a column to that line's
//! length, where a stale byte offset would land mid-sentence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Carets {
    path: PathBuf,
    // BTreeMap for the positions file's reason: someone may read this at
    // 3 AM, so saves keep a deterministic order and diffs stay clean
    remembered: BTreeMap<String, (usize, usize)>,
}

impl Carets {
    /// Read the whole file once, when the shell mounts.
    ///
    /// Never an error, and never a save-blocker: a missing or unreadable
    /// file is "nothing remembered", and a malformed line loses that line,
    /// not the file.
    pub fn load(path: &Path) -> Carets {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Carets {
            path: path.to_path_buf(),
            remembered: text.lines().filter_map(parse_line).collect(),
        }
    }

    /// Where a note was left, `None` for one never opened.
    pub fn get(&self, key: &str) -> Option<(usize, usize)> {
        self.remembered.get(key).copied()
    }

    /// The note is being left: this is where its caret stood.
    ///
    /// A key that cannot be spelled in this format — empty, or carrying
    /// whitespace the reader would split on — is not remembered rather
    /// than written as a line the next load would drop.
    pub fn set(&mut self, key: &str, line: usize, column: usize) {
        if key.is_empty() || key.contains(char::is_whitespace) {
            return;
        }
        self.remembered.insert(key.to_string(), (line, column));
    }

    /// Drop-on-delete, the positions store's rule mirrored: a deleted note
    /// remembers nothing, and a note recreated under the same path opens
    /// like one nobody has opened.
    pub fn remove(&mut self, key: &str) {
        self.remembered.remove(key);
    }

    /// Write-temp-then-rename through the persist seam
    /// (adr/2026-08-atomic-persist-seam.md): a crash mid-save leaves the
    /// previous places, never a truncated file. The parent is ensured
    /// first — a note can be left before the first survey has created
    /// `.index/`.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let lines: String = self
            .remembered
            .iter()
            .map(|(key, (line, column))| format!("{key} {line} {column}\n"))
            .collect();
        self.path
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()?;
        crate::persist::write_atomic(&self.path, &lines).map(|_| ())
    }
}

/// One entry per line, `path line column`, whitespace-separated. The key
/// is the note's vault-relative path — not its id, because a note with no
/// `#meta` has no id and still deserves to reopen where it was left
/// (adr/2026-09-a-note-reopens-where-it-was-left.md). Anything else —
/// wrong field count, a non-numeric or negative number — is not an entry.
fn parse_line(line: &str) -> Option<(String, (usize, usize))> {
    let mut fields = line.split_whitespace();
    let key = fields.next()?;
    let at: usize = fields.next()?.parse().ok()?;
    let column: usize = fields.next()?.parse().ok()?;
    if fields.next().is_some() {
        return None;
    }
    Some((key.to_string(), (at, column)))
}

/// Where a note opens: the remembered place clamped into the text it has
/// now, or the first-open landing when nothing is remembered.
pub fn landing(text: &str, remembered: Option<(usize, usize)>) -> usize {
    match remembered {
        Some((line, column)) => place(text, line, column),
        None => first_open(text),
    }
}

/// The byte offset a remembered `(line, column)` names in `text`: a line
/// past the end clamps to the last line, a column past the end of its line
/// to that line's end. Never panics and never lands off a char boundary,
/// so a note rewritten outside the app between two visits still opens.
pub fn place(text: &str, line: usize, column: usize) -> usize {
    let (start, body) = nth_line(text, line);
    start
        + body
            .char_indices()
            .nth(column)
            .map(|(at, _)| at)
            .unwrap_or(body.len())
}

/// The 0-based line and the column in chars of that line where `offset`
/// sits — the pair the store writes. An offset past the end of the text,
/// or off a char boundary, degrades to the nearest place before it.
pub fn locate(text: &str, offset: usize) -> (usize, usize) {
    let offset = (0..=offset.min(text.len()))
        .rev()
        .find(|&at| text.is_char_boundary(at))
        .unwrap_or(0);
    let before = &text[..offset];
    let start = before.rfind('\n').map(|at| at + 1).unwrap_or(0);
    (
        before.bytes().filter(|byte| *byte == b'\n').count(),
        before[start..].chars().count(),
    )
}

/// Where a note nobody has opened opens: the end of its title heading —
/// the first line starting with `= `, past the `#import`/`#meta` preamble
/// every note carries — else the end of the first line with anything on
/// it, else the start of the note.
pub fn first_open(text: &str) -> usize {
    let heading = line_ends(text).find(|(body, _)| body.starts_with("= "));
    let written = || line_ends(text).find(|(body, _)| !body.trim().is_empty());
    heading.or_else(written).map(|(_, end)| end).unwrap_or(0)
}

/// Each line's text and the byte offset just past it, newline excluded.
fn line_ends(text: &str) -> impl Iterator<Item = (&str, usize)> {
    text.split('\n').scan(0, |at, body| {
        let end = *at + body.len();
        *at = end + 1;
        Some((body, end))
    })
}

/// The byte start of line `line` and its text without the newline, the
/// last line when the note has fewer lines than that. `split` always
/// yields one item, so the walk always assigns.
fn nth_line(text: &str, line: usize) -> (usize, &str) {
    let mut at = 0;
    let mut found = (0, "");
    for (index, body) in text.split('\n').enumerate() {
        if index > line {
            break;
        }
        found = (at, body);
        at += body.len() + 1;
    }
    found
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> Carets {
        Carets::load(&dir.path().join("carets"))
    }

    #[test]
    fn places_roundtrip_through_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut carets = store(&dir);
        carets.set("permanent/luhmann.typ", 12, 4);
        carets.set("time/2026-07-24.typ", 0, 0);
        carets.save().expect("save");

        let reloaded = store(&dir);
        assert_eq!(reloaded.get("permanent/luhmann.typ"), Some((12, 4)));
        assert_eq!(reloaded.get("time/2026-07-24.typ"), Some((0, 0)));
        assert_eq!(reloaded.get("permanent/never-opened.typ"), None);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("carets"))
                .expect("read back"),
            "permanent/luhmann.typ 12 4\ntime/2026-07-24.typ 0 0\n"
        );
    }

    #[test]
    fn a_missing_file_means_nothing_remembered() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(store(&dir).get("permanent/luhmann.typ"), None);
    }

    #[test]
    fn a_malformed_file_degrades_to_nothing_remembered() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("carets"), b"\xff\xfe not a file")
            .expect("write garbage");
        assert_eq!(store(&dir).get("permanent/luhmann.typ"), None);
    }

    #[test]
    fn a_malformed_line_loses_that_line_not_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("carets"),
            "kept.typ 3 7\n\
             \n\
             only-a-path.typ\n\
             missing-column.typ 3\n\
             not-numeric.typ three 7\n\
             column-not-numeric.typ 3 seven\n\
             negative.typ -3 7\n\
             too-many.typ 3 7 9\n\
             also-kept.typ 0 0\n",
        )
        .expect("write mixed file");

        let carets = store(&dir);
        assert_eq!(carets.get("kept.typ"), Some((3, 7)));
        assert_eq!(carets.get("also-kept.typ"), Some((0, 0)));
        for orphan in [
            "only-a-path.typ",
            "missing-column.typ",
            "not-numeric.typ",
            "column-not-numeric.typ",
            "negative.typ",
            "too-many.typ",
        ] {
            assert_eq!(carets.get(orphan), None, "{orphan}");
        }
    }

    #[test]
    fn unknown_paths_ride_along_and_survive_a_save() {
        // the store never consults the index: an entry whose note was
        // deleted outside the app, or hand-added, is preserved
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("carets"), "gone.typ 1 2\n")
            .expect("write entry");

        let mut carets = store(&dir);
        carets.set("here.typ", 3, 4);
        carets.save().expect("save");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("carets"))
                .expect("read back"),
            "gone.typ 1 2\nhere.typ 3 4\n"
        );
    }

    #[test]
    fn a_key_this_format_cannot_spell_is_not_remembered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut carets = store(&dir);
        carets.set("", 1, 2);
        carets.set("permanent/two words.typ", 1, 2);
        carets.save().expect("save");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("carets"))
                .expect("read back"),
            ""
        );
    }

    #[test]
    fn remove_drops_the_entry_from_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut carets = store(&dir);
        carets.set("deleted.typ", 1, 2);
        carets.set("kept.typ", 3, 4);
        carets.remove("deleted.typ");
        carets.save().expect("save");

        let reloaded = store(&dir);
        assert_eq!(reloaded.get("deleted.typ"), None);
        assert_eq!(reloaded.get("kept.typ"), Some((3, 4)));
    }

    #[test]
    fn save_writes_atomically_and_leaves_no_litter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut carets = store(&dir);
        carets.set("here.typ", 0, 0);
        carets.save().expect("save");
        assert!(
            !dir.path().join("carets.tmp").exists(),
            "the temp took the path, it did not stay beside it"
        );
    }

    #[test]
    fn save_recreates_a_missing_index_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".index/carets");
        Carets::load(&path)
            .save()
            .expect("the save creates its parent");
        assert!(path.exists());
    }

    /// The positions store's twin: parent creation is refused by locking
    /// the grandparent. The lock is lifted before the tempdir drops.
    fn lock(dir: &Path, readonly: bool) {
        let mut permissions = std::fs::metadata(dir)
            .expect("the dir exists")
            .permissions();
        permissions.set_readonly(readonly);
        std::fs::set_permissions(dir, permissions)
            .expect("the dir permissions are set");
    }

    #[test]
    fn save_reports_a_parent_that_cannot_be_created() {
        let dir = tempfile::tempdir().expect("tempdir");
        lock(dir.path(), true);
        let carets = Carets::load(&dir.path().join("no-such-dir/carets"));
        assert!(carets.save().is_err());
        lock(dir.path(), false);
    }

    /// The preamble every note carries, a title with a two-byte glyph in
    /// it, and one line of prose.
    const NOTE: &str = "#import \"/t.typ\": *\n\
                        #meta(\n\
                        \x20 id: \"a\",\n\
                        )\n\
                        \n\
                        = Le café\n\
                        \n\
                        une ligne\n";

    #[test]
    fn locate_names_the_line_and_the_column_in_chars() {
        assert_eq!(locate(NOTE, 0), (0, 0));
        // the é is two bytes and one column
        let head = NOTE.find("café").expect("the heading") + "caf".len();
        assert_eq!(locate(NOTE, head), (5, 8));
        assert_eq!(locate(NOTE, NOTE.len()), (8, 0));
    }

    #[test]
    fn locate_degrades_past_the_end_and_off_a_boundary() {
        // a stale offset from a longer version of the note
        assert_eq!(locate(NOTE, NOTE.len() + 500), (8, 0));
        // mid-é: the nearest boundary before it
        let inside = NOTE.find("café").expect("the heading") + "caf".len() + 1;
        assert_eq!(locate(NOTE, inside), (5, 8));
    }

    #[test]
    fn place_is_locate_backwards() {
        for offset in [0, 20, 45, NOTE.len()] {
            let (line, column) = locate(NOTE, offset);
            assert_eq!(place(NOTE, line, column), offset, "at {offset}");
        }
    }

    #[test]
    fn place_clamps_a_line_and_a_column_the_note_no_longer_has() {
        // the note shrank under the memory: the last line takes it
        assert_eq!(place(NOTE, 900, 0), NOTE.len());
        // the line shrank: its end takes it
        assert_eq!(
            place(NOTE, 1, 900),
            NOTE.find("\n  id").expect("the second line")
        );
        assert_eq!(place("", 3, 4), 0);
    }

    #[test]
    fn a_note_nobody_opened_lands_at_the_end_of_its_title() {
        let at = first_open(NOTE);
        assert_eq!(locate(NOTE, at), (5, 9));
        assert!(NOTE[..at].ends_with("\n= Le café"));
    }

    #[test]
    fn a_note_with_no_heading_lands_at_the_end_of_its_first_written_line() {
        let text = "\n\nune ligne\nune autre\n";
        assert_eq!(locate(text, first_open(text)), (2, 9));
    }

    #[test]
    fn an_empty_note_lands_at_its_start() {
        assert_eq!(first_open(""), 0);
        assert_eq!(first_open("\n  \n\n"), 0);
    }

    #[test]
    fn landing_prefers_the_memory_and_falls_back_to_the_title() {
        assert_eq!(landing(NOTE, Some((7, 4))), place(NOTE, 7, 4));
        assert_eq!(landing(NOTE, None), first_open(NOTE));
    }
}
