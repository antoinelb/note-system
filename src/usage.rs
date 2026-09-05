//! Palette usage: command → how many times it was run, in a plain-lines
//! file under `.index/`, beside the positions file and never inside the
//! database, so rebuilding the index cannot lose it
//! (adr/2026-09-palette-orders-by-usage.md).
//!
//! Nothing derives these counts — they are the record of what the user
//! reached for, the same class of user data as a canvas position
//! (adr/2026-07-positions-separate-file.md). A missing or unreadable file
//! is "nothing counted yet", never an error: the palette then shows the
//! alphabetical order a fresh install shows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::palette::CommandId;

pub struct Usage {
    path: PathBuf,
    // BTreeMap for the positions file's reason: someone may read this at
    // 3 AM, so saves keep a deterministic order and diffs stay clean
    counted: BTreeMap<String, u32>,
}

impl Usage {
    /// Read the whole file once, when the shell mounts.
    ///
    /// Never an error, and never a save-blocker: a missing or unreadable
    /// file is "nothing counted yet", and a malformed line loses that line,
    /// not the file.
    pub fn load(path: &Path) -> Usage {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Usage {
            path: path.to_path_buf(),
            counted: text.lines().filter_map(parse_line).collect(),
        }
    }

    /// How often a command was run — zero for one never run, which is the
    /// same answer as one the file never named.
    pub fn count(&self, id: CommandId) -> u32 {
        self.counted.get(key(id)).copied().unwrap_or(0)
    }

    /// One run. Saturating, so a count that somehow reached `u32::MAX`
    /// stops climbing instead of wrapping back to the bottom of the list.
    pub fn record(&mut self, id: CommandId) {
        let slot = self.counted.entry(key(id).to_string()).or_insert(0);
        *slot = slot.saturating_add(1);
    }

    /// Write-temp-then-rename through the persist seam
    /// (adr/2026-08-atomic-persist-seam.md): a crash mid-save leaves the
    /// previous counts, never a truncated file. The parent is ensured
    /// first — a command can be run before the first survey has created
    /// `.index/`.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let lines: String = self
            .counted
            .iter()
            .map(|(id, count)| format!("{id} {count}\n"))
            .collect();
        self.path
            .parent()
            .map(std::fs::create_dir_all)
            .transpose()?;
        crate::persist::write_atomic(&self.path, &lines).map(|_| ())
    }
}

/// One entry per line, `command count`, whitespace-separated. Keys never
/// contain spaces, so the format needs no quoting. Anything else — wrong
/// field count, a non-numeric or negative count — is not an entry.
fn parse_line(line: &str) -> Option<(String, u32)> {
    let mut fields = line.split_whitespace();
    let id = fields.next()?;
    let count: u32 = fields.next()?.parse().ok()?;
    if fields.next().is_some() {
        return None;
    }
    Some((id.to_string(), count))
}

/// The stable name a command is counted under: the `CommandId`, not the
/// label, so renaming a row keeps its history
/// (adr/2026-09-palette-orders-by-usage.md). Exhaustive on purpose — a
/// variant added to the registry does not compile until it is named here.
fn key(id: CommandId) -> &'static str {
    match id {
        CommandId::ToggleTheme => "toggle-theme",
        CommandId::Quit => "quit",
        CommandId::SearchText => "search-text",
        CommandId::InsertLink => "insert-link",
        CommandId::FollowLink => "follow-link",
        CommandId::OpenLoops => "open-loops",
        CommandId::OpenDaily => "open-daily",
        CommandId::PreviousDaily => "previous-daily",
        CommandId::NextDaily => "next-daily",
        CommandId::OpenWeekly => "open-weekly",
        CommandId::PreviousWeekly => "previous-weekly",
        CommandId::NextWeekly => "next-weekly",
        CommandId::OpenSeason => "open-season",
        CommandId::PreviousSeason => "previous-season",
        CommandId::NextSeason => "next-season",
        CommandId::GoToTable => "go-to-table",
        CommandId::GoToLogs => "go-to-logs",
        CommandId::NewNote => "new-note",
        CommandId::DeleteNote => "delete-note",
        CommandId::Notices => "notices",
        CommandId::KeepMine => "keep-mine",
        CommandId::TakeDisk => "take-disk",
        CommandId::ZoomToBodies => "zoom-to-bodies",
        CommandId::ZoomToTitles => "zoom-to-titles",
        CommandId::FilterCards => "filter-cards",
        CommandId::FoldRail => "fold-rail",
        CommandId::FoldJump => "fold-jump",
        CommandId::OpenNote => "open-note",
        CommandId::ArrangeCluster => "arrange-cluster",
        CommandId::Undo => "undo",
        CommandId::EditTemplate => "edit-template",
        CommandId::ExportPdf => "export-pdf",
        CommandId::OpenSettings => "open-settings",
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::palette::COMMANDS;

    fn store(dir: &tempfile::TempDir) -> Usage {
        Usage::load(&dir.path().join("usage"))
    }

    #[test]
    fn counts_roundtrip_through_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut usage = store(&dir);
        usage.record(CommandId::ToggleTheme);
        usage.record(CommandId::ToggleTheme);
        usage.record(CommandId::OpenDaily);
        usage.save().expect("save");

        let reloaded = store(&dir);
        assert_eq!(reloaded.count(CommandId::ToggleTheme), 2);
        assert_eq!(reloaded.count(CommandId::OpenDaily), 1);
        assert_eq!(reloaded.count(CommandId::Quit), 0);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("usage"))
                .expect("read back"),
            "open-daily 1\ntoggle-theme 2\n"
        );
    }

    #[test]
    fn a_missing_file_means_nothing_counted() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(store(&dir).count(CommandId::ToggleTheme), 0);
    }

    #[test]
    fn a_malformed_file_degrades_to_nothing_counted() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("usage"), b"\xff\xfe not a file")
            .expect("write garbage");
        assert_eq!(store(&dir).count(CommandId::ToggleTheme), 0);
    }

    #[test]
    fn a_malformed_line_loses_that_line_not_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("usage"),
            "toggle-theme 4\n\
             \n\
             only-a-command\n\
             not-numeric many\n\
             negative -1\n\
             too-many 1 2\n\
             open-daily 7\n",
        )
        .expect("write mixed file");

        let usage = store(&dir);
        assert_eq!(usage.count(CommandId::ToggleTheme), 4);
        assert_eq!(usage.count(CommandId::OpenDaily), 7);
        for orphan in ["only-a-command", "not-numeric", "negative", "too-many"]
        {
            assert!(!usage.counted.contains_key(orphan), "{orphan}");
        }
    }

    #[test]
    fn unknown_keys_ride_along_and_survive_a_save() {
        // the store never consults the registry: a key from an older build,
        // or a hand-added line, is tolerated and preserved
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("usage"), "no-such-command 3\n")
            .expect("write entry");

        let mut usage = store(&dir);
        usage.record(CommandId::Quit);
        usage.save().expect("save");

        assert_eq!(
            std::fs::read_to_string(dir.path().join("usage"))
                .expect("read back"),
            "no-such-command 3\nquit 1\n"
        );
    }

    #[test]
    fn a_count_at_the_ceiling_stops_instead_of_wrapping() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("usage"),
            format!("toggle-theme {}\n", u32::MAX),
        )
        .expect("write the ceiling");

        let mut usage = store(&dir);
        usage.record(CommandId::ToggleTheme);
        assert_eq!(usage.count(CommandId::ToggleTheme), u32::MAX);
    }

    #[test]
    fn save_writes_atomically_and_leaves_no_litter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut usage = store(&dir);
        usage.record(CommandId::Notices);
        usage.save().expect("save");
        assert!(
            !dir.path().join("usage.tmp").exists(),
            "the temp took the path, it did not stay beside it"
        );
    }

    #[test]
    fn save_recreates_a_missing_index_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".index/usage");
        Usage::load(&path)
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
        let usage = Usage::load(&dir.path().join("no-such-dir/usage"));
        assert!(usage.save().is_err());
        lock(dir.path(), false);
    }

    /// Every command gets a key, and no two share one — a collision would
    /// silently pool two commands' histories into one count.
    #[test]
    fn every_command_has_its_own_stable_key() {
        let mut keys: Vec<&str> =
            COMMANDS.iter().map(|command| key(command.id)).collect();
        assert_eq!(keys.len(), COMMANDS.len());
        for name in &keys {
            assert!(
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '-'),
                "{name} is not a kebab-case key"
            );
        }
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), COMMANDS.len(), "keys must be unique");
    }
}
