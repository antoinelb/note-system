//! The one seam under every byte the app writes: a note, the positions
//! file and a created note all land whole or not at all
//! (adr/2026-08-atomic-persist-seam.md). This closes the crash window the
//! debounced-autosave ADR recorded as a known ceiling — a kill mid-write
//! can no longer truncate a file that already held data.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Replace `path` with `text` atomically: the bytes land in a sibling
/// `.tmp` file, reach the disk, and take the path by rename — a crash at
/// any point leaves the previous version on disk, never a truncation.
///
/// Returns the written file's mtime — the stamp the editor's external-edit
/// guard compares against. It is read from the temp file before the
/// rename, which preserves it, so no other writer can slip between the
/// write and the stat.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<SystemTime> {
    let tmp = sibling(path);
    File::create(&tmp)
        .and_then(|file| fill(file, text))
        .and_then(|stamp| fs::rename(&tmp, path).map(|()| stamp))
        .inspect_err(|_| {
            // best effort: an unrenamed temp is litter, never data
            let _ = fs::remove_file(&tmp);
        })
}

/// Create `path` refusing to replace anything: `create_new` makes the
/// existence check and the creation one operation, so two writers — the
/// app and a headless `--capture`, say — cannot both pass a pre-check and
/// clobber each other the way the old `exists()`-then-write could.
pub fn create_new(path: &Path, text: &str) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|file| fill(file, text).map(|_| ()))
}

/// The write every path shares: all the bytes, synced to the disk. Without
/// the sync, a crash shortly after the rename can still surface an empty
/// file — the very truncation this module exists to close.
fn fill(mut file: File, text: &str) -> io::Result<SystemTime> {
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| file.metadata())
        .and_then(|meta| meta.modified())
}

/// The temp beside its target — same directory, so the rename never
/// crosses a filesystem; non-`.typ`, so the watcher and the vault scan
/// never see it (`watch::note_path`).
fn sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_the_file_and_stamps_its_mtime() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.typ");
        fs::write(&path, "before").expect("the old version is written");

        let stamp = write_atomic(&path, "after").expect("the write lands");
        assert_eq!(
            fs::read_to_string(&path).expect("the file is readable"),
            "after"
        );
        let disk = fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .expect("the file has an mtime");
        assert_eq!(stamp, disk, "the rename preserved the stamp");
        assert!(
            !sibling(&path).exists(),
            "the temp took the path, it did not stay beside it"
        );
    }

    #[test]
    fn a_missing_directory_is_an_error_and_leaves_no_litter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("no-such-dir/note.typ");
        assert!(write_atomic(&path, "text").is_err());
        assert!(!sibling(&path).exists());
    }

    #[test]
    fn a_refused_rename_reports_and_removes_its_temp() {
        // the target squatted by a directory: the temp writes, the rename
        // refuses, and the cleanup arm sweeps the temp away
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.typ");
        fs::create_dir(&path).expect("the squatter is created");

        assert!(write_atomic(&path, "text").is_err());
        assert!(!sibling(&path).exists(), "the temp was swept");
        assert!(path.is_dir(), "the squatter is untouched");
    }

    #[test]
    fn create_new_writes_a_file_that_was_not_there() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.typ");
        create_new(&path, "= fresh\n").expect("the create lands");
        assert_eq!(
            fs::read_to_string(&path).expect("the file is readable"),
            "= fresh\n"
        );
    }

    #[test]
    fn create_new_refuses_an_existing_file_and_leaves_it_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("note.typ");
        fs::write(&path, "the original").expect("the original is written");

        let error = create_new(&path, "an impostor").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error}");
        assert_eq!(
            fs::read_to_string(&path).expect("the file is readable"),
            "the original"
        );
    }

    #[test]
    fn create_new_reports_a_missing_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(create_new(&dir.path().join("no/note.typ"), "x").is_err());
    }
}
