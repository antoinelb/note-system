//! Watcher integration tests (phase 3): real inotify events on a temp vault.
//! Exit criterion under test: an edit on disk reaches the index without a
//! rebuild, and anything the watcher cannot attribute still lands on one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use note_system::domain::{NoteCategory, NoteId, NoteType};
use note_system::index::{Index, scan_vault};
use note_system::watch::{VaultChange, VaultWatcher, apply};

/// Long enough for inotify plus the 200 ms debounce, short enough that a
/// genuine failure fails the suite instead of hanging it.
const DELIVERY: Duration = Duration::from_secs(5);

/// How long the watcher must stay silent before we call a burst finished.
const QUIET_GAP: Duration = Duration::from_millis(750);

/// One filesystem operation can span several debounce windows; more than this
/// many means the watcher is looping on its own output.
const BATCH_LIMIT: usize = 16;

// ---------------------------------------------------------------- round trip

#[test]
fn writing_a_note_reaches_the_index_through_the_watcher() {
    let (dir, mut index) = temp_vault();
    let watcher = VaultWatcher::start(dir.path()).expect("start watcher");

    write_note(
        dir.path(),
        "permanent/added.typ",
        "added",
        "concept",
        &[],
        &[],
    );
    drain(&watcher, &mut index, dir.path());

    assert_eq!(
        index.notes_by_type(&NoteType::Concept).expect("by type"),
        paths(&["permanent/added.typ"])
    );
}

#[test]
fn deleting_a_note_removes_it_from_the_index() {
    let (dir, mut index) = seeded_vault();
    let watcher = VaultWatcher::start(dir.path()).expect("start watcher");

    std::fs::remove_file(dir.path().join("permanent/seed.typ"))
        .expect("delete note");
    drain(&watcher, &mut index, dir.path());

    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("by category"),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn renaming_a_note_moves_it_in_the_index() {
    let (dir, mut index) = seeded_vault();
    let watcher = VaultWatcher::start(dir.path()).expect("start watcher");

    std::fs::rename(
        dir.path().join("permanent/seed.typ"),
        dir.path().join("permanent/renamed.typ"),
    )
    .expect("rename note");
    drain(&watcher, &mut index, dir.path());

    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("by category"),
        paths(&["permanent/renamed.typ"])
    );
}

#[test]
fn rewriting_a_note_drops_the_rows_of_its_previous_contents() {
    // the incremental path deletes before it inserts and relies on
    // ON DELETE CASCADE for tags, links and anomalies; a stale tag row here
    // would mean the cascade is not actually enforced
    let (dir, mut index) = seeded_vault();
    let watcher = VaultWatcher::start(dir.path()).expect("start watcher");

    write_note(dir.path(), "permanent/seed.typ", "seed", "claim", &[], &[]);
    drain(&watcher, &mut index, dir.path());

    assert_eq!(
        index.notes_by_tag("method").expect("by tag"),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        index
            .backlinks(&NoteId("elsewhere".into()))
            .expect("backlinks"),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        index.notes_by_type(&NoteType::Claim).expect("by type"),
        paths(&["permanent/seed.typ"])
    );
}

// ---------------------------------------------------------------- no feedback

#[test]
fn index_writes_do_not_wake_the_watcher() {
    // `.index/` lives inside the vault, so SQLite's own writes reach the
    // watcher; if they counted as changes each rebuild would trigger the next
    let (dir, mut index) = temp_vault();
    let watcher = VaultWatcher::start(dir.path()).expect("start watcher");

    index
        .rebuild(&scan_vault(dir.path()).expect("scan empty vault"))
        .expect("rebuild writes into .index/");

    assert!(
        watcher.changes.recv_timeout(QUIET_GAP).is_err(),
        "a rebuild must not report itself as a vault change"
    );
}

#[test]
fn editing_a_template_does_not_wake_the_watcher() {
    let (dir, _index) = temp_vault();
    let watcher = VaultWatcher::start(dir.path()).expect("start watcher");

    std::fs::write(dir.path().join("templates/daily.typ"), "#let daily = 1")
        .expect("write template");

    assert!(
        watcher.changes.recv_timeout(QUIET_GAP).is_err(),
        "templates/ is not a note category"
    );
}

// ---------------------------------------------------------------- fallbacks

#[test]
fn a_rescan_falls_back_to_a_full_rebuild() {
    let (dir, mut index) = temp_vault();
    write_note(
        dir.path(),
        "permanent/unseen.typ",
        "unseen",
        "concept",
        &[],
        &[],
    );

    apply(&mut index, dir.path(), &[VaultChange::Rescan]).expect("rescan");

    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("by category"),
        paths(&["permanent/unseen.typ"])
    );
}

#[test]
fn a_note_gone_before_the_change_is_applied_is_removed_not_an_error() {
    // a file created and deleted inside one debounce window is reported as
    // touched, and the file is already gone by the time we read it
    let (dir, mut index) = seeded_vault();

    std::fs::remove_file(dir.path().join("permanent/seed.typ"))
        .expect("delete note");
    let touched = VaultChange::Touched {
        category: NoteCategory::Permanent,
        path: PathBuf::from("permanent/seed.typ"),
    };
    apply(&mut index, dir.path(), &[touched]).expect("touch a missing file");

    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("by category"),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn a_rescan_ends_the_batch() {
    // changes queued after a rescan are covered by the rebuild it triggers,
    // and applying them afterwards would write on top of it
    let (dir, mut index) = temp_vault();
    write_note(
        dir.path(),
        "permanent/real.typ",
        "real",
        "concept",
        &[],
        &[],
    );

    let changes = [
        VaultChange::Rescan,
        VaultChange::Removed(PathBuf::from("permanent/real.typ")),
    ];
    apply(&mut index, dir.path(), &changes).expect("rescan then removal");

    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("by category"),
        paths(&["permanent/real.typ"])
    );
}

#[test]
fn a_rescan_reports_a_vault_it_cannot_scan() {
    let (dir, mut index) = temp_vault();
    let gone = dir.path().join("no-such-vault");
    assert!(apply(&mut index, &gone, &[VaultChange::Rescan]).is_err());
}

#[test]
fn a_note_that_cannot_be_read_is_reported_not_swallowed() {
    // only a missing file is treated as a removal; every other read failure
    // is a real failure and must reach the caller
    let (dir, mut index) = temp_vault();
    std::fs::write(
        dir.path().join("permanent/broken.typ"),
        [0xff, 0xfe, 0x00],
    )
    .expect("write invalid utf-8");

    let touched = VaultChange::Touched {
        category: NoteCategory::Permanent,
        path: PathBuf::from("permanent/broken.typ"),
    };
    assert!(apply(&mut index, dir.path(), &[touched]).is_err());
}

#[test]
fn a_removal_reports_index_failures() {
    let (dir, mut index) = seeded_vault();
    {
        let raw =
            rusqlite::Connection::open(dir.path().join(".index/index.db"))
                .expect("raw open");
        raw.execute_batch("DROP TABLE notes;").expect("drop notes");
    }

    let removed = VaultChange::Removed(PathBuf::from("permanent/seed.typ"));
    assert!(apply(&mut index, dir.path(), &[removed]).is_err());
}

#[test]
fn watching_a_vault_that_does_not_exist_reports_the_error() {
    let dir = tempdir();
    assert!(VaultWatcher::start(&dir.path().join("no-such-vault")).is_err());
}

// ---------------------------------------------------------------- helpers

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("create tempdir")
}

/// A vault with the four category directories, `templates/`, and an index
/// living inside it at `.index/index.db` — the real layout, so the tests see
/// the same self-triggering hazard the app will.
fn temp_vault() -> (tempfile::TempDir, Index) {
    let dir = tempdir();
    for name in [
        ".index",
        "templates",
        "permanent",
        "time",
        "capture",
        "generated",
    ] {
        std::fs::create_dir(dir.path().join(name)).expect("create vault dir");
    }
    let index = Index::open(&dir.path().join(".index/index.db"))
        .expect("open fresh index");
    (dir, index)
}

/// A vault holding one tagged, linking note, already indexed.
fn seeded_vault() -> (tempfile::TempDir, Index) {
    let (dir, mut index) = temp_vault();
    write_note(
        dir.path(),
        "permanent/seed.typ",
        "seed",
        "concept",
        &["method"],
        &["elsewhere"],
    );
    index
        .rebuild(&scan_vault(dir.path()).expect("scan seeded vault"))
        .expect("rebuild seeded vault");
    (dir, index)
}

fn write_note(
    root: &Path,
    relative: &str,
    id: &str,
    note_type: &str,
    tags: &[&str],
    links: &[&str],
) {
    let tags: String = tags.iter().map(|tag| format!("\"{tag}\", ")).collect();
    let body: String = links
        .iter()
        .map(|link| format!("Voir #l(\"{link}\").\n"))
        .collect();
    std::fs::write(
        root.join(relative),
        format!(
            "#meta(id: \"{id}\", type: \"{note_type}\", tags: ({tags}))\n{body}"
        ),
    )
    .expect("write note");
}

/// Apply every batch the watcher sends, stopping at the first quiet gap.
///
/// One filesystem operation can span more than one debounce window, so a
/// single `recv_timeout` is not enough. The loop is bounded so that a watcher
/// reacting to its own effects fails the test instead of hanging it.
fn drain(watcher: &VaultWatcher, index: &mut Index, root: &Path) {
    // every completed iteration applies exactly one batch, so the loop
    // variable is the count of batches applied so far
    for batches in 0..BATCH_LIMIT {
        let patience = if batches == 0 { DELIVERY } else { QUIET_GAP };
        let Ok(changes) = watcher.changes.recv_timeout(patience) else {
            assert!(batches > 0, "the watcher reported nothing at all");
            return;
        };
        apply(index, root, &changes).expect("apply changes");
    }
    panic!("the watcher never went quiet after {BATCH_LIMIT} batches");
}

fn paths(relative: &[&str]) -> Vec<PathBuf> {
    relative.iter().map(PathBuf::from).collect()
}
