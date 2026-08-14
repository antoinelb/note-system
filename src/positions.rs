//! Canvas positions: id → (x, y) in a plain-lines file under `.index/`,
//! beside the database and never inside it, so rebuilding the index cannot
//! lose them (adr/2026-07-positions-separate-file.md,
//! adr/2026-08-positions-plain-lines-file.md).
//!
//! A missing entry means "not yet placed" — never an error; auto-placement
//! (v1 phase 8) decides later. The store is synchronous like `Editor::save`;
//! the debounced write timer belongs to the UI that mounts the table.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Positions {
    path: PathBuf,
    // BTreeMap: the file is user data someone may read at 3 AM, so saves
    // keep a deterministic order and diffs stay clean
    placed: BTreeMap<String, (f64, f64)>,
}

impl Positions {
    /// Read the whole file once, when the table loads.
    ///
    /// Never an error: a missing or unreadable file is "nothing placed yet",
    /// and a malformed line loses that line, not the file — the notes
    /// themselves are untouched either way.
    pub fn load(path: &Path) -> Positions {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let placed = text.lines().filter_map(parse_line).collect();
        Positions {
            path: path.to_path_buf(),
            placed,
        }
    }

    pub fn get(&self, id: &str) -> Option<(f64, f64)> {
        self.placed.get(id).copied()
    }

    pub fn set(&mut self, id: &str, x: f64, y: f64) {
        self.placed.insert(id.to_string(), (x, y));
    }

    /// Drop-on-delete: the delete path removes the entry, and a recreated id
    /// starts unplaced (adr/2026-08-position-dropped-on-delete.md).
    pub fn remove(&mut self, id: &str) {
        self.placed.remove(id);
    }

    /// Every placed entry, in the file's deterministic order — the anchors
    /// auto-placement clusters around
    /// (adr/2026-08-auto-place-strongest-link-ring.md).
    pub fn iter(&self) -> impl Iterator<Item = (&str, (f64, f64))> {
        self.placed.iter().map(|(id, at)| (id.as_str(), *at))
    }

    /// Write-temp-then-rename through the persist seam: the ceiling the
    /// debounced-autosave ADR recorded is closed — a crash mid-save leaves
    /// the previous placements, never a truncated file
    /// (adr/2026-08-atomic-persist-seam.md). The parent is ensured first:
    /// positions are user data living in `.index/`, and on a threaded
    /// launch the debounce can fire before the first survey creates that
    /// directory (adr/2026-08-startup-survey-async.md).
    pub fn save(&self) -> Result<(), std::io::Error> {
        let lines: String = self
            .placed
            .iter()
            .map(|(id, (x, y))| format!("{id} {x} {y}\n"))
            .collect();
        self.path
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()?;
        crate::persist::write_atomic(&self.path, &lines).map(|_| ())
    }
}

/// One entry per line, `id x y`, whitespace-separated; ids are kebab-case
/// (adr/2026-07-id-scheme-kebab-frozen.md) so they never contain spaces.
/// Anything else — wrong field count, non-numeric or non-finite coordinates —
/// is not an entry.
fn parse_line(line: &str) -> Option<(String, (f64, f64))> {
    let mut fields = line.split_whitespace();
    let id = fields.next()?;
    let x: f64 = fields.next()?.parse().ok()?;
    let y: f64 = fields.next()?.parse().ok()?;
    // "nan nan" parses as f64 but is not a place on the table
    if fields.next().is_some() || !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some((id.to_string(), (x, y)))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> Positions {
        Positions::load(&dir.path().join("positions"))
    }

    #[test]
    fn positions_roundtrip_through_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut positions = store(&dir);
        positions.set("deep-modules", 340.0, -120.5);
        positions.set("zettelkasten", -510.25, 40.0);
        positions.save().expect("save");

        let reloaded = store(&dir);
        assert_eq!(reloaded.get("deep-modules"), Some((340.0, -120.5)));
        assert_eq!(reloaded.get("zettelkasten"), Some((-510.25, 40.0)));
        assert_eq!(reloaded.get("never-placed"), None);
    }

    #[test]
    fn a_missing_file_means_nothing_placed() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(store(&dir).get("anything"), None);
    }

    #[test]
    fn a_malformed_file_degrades_to_nothing_placed() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("positions"), b"\xff\xfe not a file")
            .expect("write garbage");
        assert_eq!(store(&dir).get("anything"), None);
    }

    #[test]
    fn a_malformed_line_loses_that_line_not_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("positions"),
            "kept 10 20\n\
             \n\
             only-an-id\n\
             missing-y 10\n\
             not-numeric ten 20\n\
             y-not-numeric 10 twenty\n\
             too-many 10 20 30\n\
             not-a-place nan inf\n\
             also-kept -3.5 0\n",
        )
        .expect("write mixed file");

        let positions = store(&dir);
        assert_eq!(positions.get("kept"), Some((10.0, 20.0)));
        assert_eq!(positions.get("also-kept"), Some((-3.5, 0.0)));
        for id in [
            "only-an-id",
            "missing-y",
            "not-numeric",
            "y-not-numeric",
            "too-many",
            "not-a-place",
        ] {
            assert_eq!(positions.get(id), None);
        }
    }

    #[test]
    fn unknown_ids_ride_along_and_survive_a_save() {
        // the store never consults the index: an entry whose note was deleted
        // outside the app, or hand-added, is tolerated and preserved
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("positions"), "no-such-note 1 2\n")
            .expect("write entry");

        let mut positions = store(&dir);
        positions.set("real-note", 3.0, 4.0);
        positions.save().expect("save");

        let text = std::fs::read_to_string(dir.path().join("positions"))
            .expect("read back");
        assert_eq!(text, "no-such-note 1 2\nreal-note 3 4\n");
    }

    #[test]
    fn remove_drops_the_entry_from_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut positions = store(&dir);
        positions.set("deleted-note", 1.0, 2.0);
        positions.set("kept-note", 3.0, 4.0);
        positions.remove("deleted-note");
        positions.save().expect("save");

        let reloaded = store(&dir);
        assert_eq!(reloaded.get("deleted-note"), None);
        assert_eq!(reloaded.get("kept-note"), Some((3.0, 4.0)));
    }

    #[test]
    fn save_recreates_a_missing_index_directory() {
        // the threaded launch can debounce a save before the first survey
        // creates `.index/`; the save owns its parent instead of failing
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".index/positions");
        let positions = Positions::load(&path);
        positions.save().expect("the save creates its parent");
        assert!(path.exists());
    }

    /// The editor tests' `lock` twin: parent creation is refused by
    /// locking the grandparent. Callers unlock before the tempdir drops.
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
        let positions =
            Positions::load(&dir.path().join("no-such-dir/positions"));
        assert!(positions.save().is_err());
        lock(dir.path(), false);
    }
}
