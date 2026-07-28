//! Index integration tests (phase 3): scan, schema lifecycle, full rebuild, queries.
//! Exit criterion under test: delete the index, reopen, rebuild — everything is back.

use std::path::{Path, PathBuf};

use note_system::domain::{
    Link, Meta, MetaAnomaly, MetaStatus, Note, NoteCategory, NoteId, NoteType,
};
use note_system::index::{
    DanglingLink, Index, IndexError, SCHEMA_VERSION, scan_vault,
};

// ---------------------------------------------------------------- scan

#[test]
fn scan_finds_every_note_sorted_and_skips_templates_and_non_typ_files() {
    let notes = scan_fixture();
    assert_eq!(notes.len(), 18);
    let all: Vec<&Path> = notes.iter().map(|n| n.path.as_path()).collect();
    assert!(all.contains(&Path::new("permanent/zettelkasten.typ")));
    assert!(!all.iter().any(|p| p.starts_with("templates")));
    assert!(
        !all.iter()
            .any(|p| p.extension() != Some(std::ffi::OsStr::new("typ")))
    );
    let mut sorted = all.clone();
    sorted.sort();
    assert_eq!(all, sorted, "scan_vault returns notes sorted by path");
}

#[test]
fn scan_assigns_category_from_top_level_directory() {
    let notes = scan_fixture();
    let count = |category: NoteCategory| {
        notes.iter().filter(|n| n.category == category).count()
    };
    assert_eq!(count(NoteCategory::Capture), 2);
    assert_eq!(count(NoteCategory::Generated), 1);
    assert_eq!(count(NoteCategory::Permanent), 10);
    assert_eq!(count(NoteCategory::Time), 5);
}

#[test]
fn scan_reports_unreadable_files_and_missing_roots_instead_of_skipping() {
    let dir = tempdir();
    let permanent = dir.path().join("permanent");
    std::fs::create_dir(&permanent).expect("create category dir");
    std::fs::write(permanent.join("broken.typ"), [0xff, 0xfe, 0x00])
        .expect("write invalid utf-8");
    assert!(matches!(scan_vault(dir.path()), Err(IndexError::Io(_))));
    assert!(matches!(
        scan_vault(&dir.path().join("no-such-vault")),
        Err(IndexError::Io(_))
    ));
}

#[test]
fn scan_reports_a_category_path_that_is_not_a_directory() {
    let dir = tempdir();
    std::fs::write(dir.path().join("permanent"), b"a file, not a directory")
        .expect("write fake category");
    assert!(matches!(scan_vault(dir.path()), Err(IndexError::Io(_))));
}

// ---------------------------------------------------------------- open lifecycle

#[test]
fn open_creates_an_empty_index_when_the_file_is_missing() {
    let (_dir, index) = temp_index();
    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("query empty index"),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn open_preserves_data_across_reopens_at_the_current_version() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    let mut index = Index::open(&db).expect("first open");
    index.rebuild(&scan_fixture()).expect("rebuild");
    drop(index);
    let index = Index::open(&db).expect("reopen");
    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Capture)
            .expect("query reopened index")
            .len(),
        2
    );
}

#[test]
fn open_discards_an_index_with_a_different_schema_version() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).expect("raw open");
        conn.execute_batch(
            "PRAGMA user_version = 9999; CREATE TABLE junk(x);",
        )
        .expect("seed stale db");
    }
    let index = Index::open(&db).expect("open discards and recreates");
    assert_eq!(
        index.typeless_notes().expect("query"),
        Vec::<PathBuf>::new()
    );
    let conn = rusqlite::Connection::open(&db).expect("raw reopen");
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("read version");
    assert_eq!(version, SCHEMA_VERSION);
    let junk: i32 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name = 'junk'",
            [],
            |row| row.get(0),
        )
        .expect("look for junk table");
    assert_eq!(junk, 0, "stale content must not survive a version bump");
}

#[test]
fn open_discards_a_file_that_is_not_a_database() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    std::fs::write(&db, b"this is not a sqlite database")
        .expect("write garbage");
    let index = Index::open(&db).expect("open recovers from corruption");
    assert_eq!(index.dangling_links().expect("query"), no_dangling());
}

#[test]
fn open_reports_an_unopenable_database_path() {
    let dir = tempdir();
    let nested = dir.path().join("no-such-dir").join("index.sqlite");
    assert!(matches!(Index::open(&nested), Err(IndexError::Sqlite(_))));
}

#[test]
fn open_reports_an_undeletable_stale_index_instead_of_panicking() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    std::fs::write(&db, b"this is not a sqlite database")
        .expect("write garbage");
    // unlinking needs write permission on the *directory*, not the file
    set_readonly(dir.path(), true);
    let result = Index::open(&db);
    // restore first: a read-only dir would also defeat the tempdir cleanup
    set_readonly(dir.path(), false);
    assert!(matches!(result, Err(IndexError::Io(_))));
}

// ---------------------------------------------------------------- rebuild

#[test]
fn rebuild_is_idempotent() {
    let (_dir, mut index) = temp_index();
    let notes = scan_fixture();
    index.rebuild(&notes).expect("first rebuild");
    let first = snapshot(&index);
    index.rebuild(&notes).expect("second rebuild");
    assert_eq!(snapshot(&index), first);
}

#[test]
fn rebuild_fully_replaces_previous_contents() {
    let (_dir, mut index) = temp_index();
    index.rebuild(&scan_fixture()).expect("full rebuild");
    let solo = vec![note(
        "permanent/solo.typ",
        NoteCategory::Permanent,
        Some("solo"),
        Some(NoteType::Idea),
        &[],
        &[],
    )];
    index.rebuild(&solo).expect("rebuild with one note");
    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("query"),
        paths(&["permanent/solo.typ"])
    );
    assert_eq!(
        index.notes_by_category(&NoteCategory::Time).expect("query"),
        no_paths()
    );
}

#[test]
fn rebuild_reports_sqlite_failures_instead_of_panicking() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    {
        Index::open(&db).expect("create empty index");
    }
    set_readonly(&db, true);
    let mut index =
        Index::open(&db).expect("read-only file still opens for querying");
    assert!(matches!(
        index.rebuild(&scan_fixture()),
        Err(IndexError::Sqlite(_))
    ));
}

#[test]
fn a_failed_rebuild_leaves_the_previous_index_intact() {
    let (_dir, mut index) = fixture_index();
    let before = snapshot(&index);
    let idea = |path, note_id| {
        note(
            path,
            NoteCategory::Permanent,
            Some(note_id),
            Some(NoteType::Idea),
            &[],
            &[],
        )
    };
    // two notes claiming one path violates notes.path PRIMARY KEY mid-insert
    let clashing = vec![
        idea("permanent/clash.typ", "a"),
        idea("permanent/clash.typ", "b"),
    ];
    assert!(matches!(
        index.rebuild(&clashing),
        Err(IndexError::Sqlite(_))
    ));
    assert_eq!(
        snapshot(&index),
        before,
        "the transaction rolls back, so the DELETEs never land"
    );
}

// ---------------------------------------------------------------- queries

#[test]
fn notes_by_category_lists_vault_relative_paths_sorted() {
    let (_dir, index) = fixture_index();
    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Capture)
            .expect("query"),
        paths(&[
            "capture/capture-articles-zettel.typ",
            "capture/capture-idea-canvas.typ",
        ])
    );
    assert_eq!(
        index
            .notes_by_category(&NoteCategory::Generated)
            .expect("query"),
        paths(&["generated/digest-smart-notes.typ"])
    );
}

#[test]
fn notes_by_type_follows_meta_not_directory() {
    let (_dir, index) = fixture_index();
    assert_eq!(
        index.notes_by_type(&NoteType::Daily).expect("query"),
        paths(&[
            "time/2026-07-21.typ",
            "time/2026-07-22.typ",
            "time/2026-07-23.typ",
        ])
    );
    // duplicate-meta: the first #meta call won, so it is a concept
    assert_eq!(
        index.notes_by_type(&NoteType::Concept).expect("query"),
        paths(&[
            "permanent/duplicate-meta.typ",
            "permanent/link-traps.typ",
            "permanent/zettelkasten.typ",
        ])
    );
    assert_eq!(
        index.notes_by_type(&NoteType::Weekly).expect("query"),
        paths(&["time/2026-w30.typ"])
    );
    assert_eq!(
        index.notes_by_type(&NoteType::Seasonal).expect("query"),
        paths(&["time/2026-summer.typ"])
    );
}

#[test]
fn unknown_types_are_queryable_verbatim() {
    let (_dir, mut index) = temp_index();
    let notes = vec![note(
        "permanent/typo.typ",
        NoteCategory::Permanent,
        Some("typo"),
        Some(NoteType::Unknown("concpet".to_string())),
        &[],
        &[],
    )];
    index.rebuild(&notes).expect("rebuild");
    assert_eq!(
        index
            .notes_by_type(&NoteType::Unknown("concpet".to_string()))
            .expect("query"),
        paths(&["permanent/typo.typ"])
    );
    assert_eq!(
        index.notes_by_type(&NoteType::Concept).expect("query"),
        no_paths()
    );
}

#[test]
fn notes_by_tag_matches_exactly() {
    let (_dir, index) = fixture_index();
    assert_eq!(
        index.notes_by_tag("method").expect("query"),
        paths(&["permanent/atomic-notes.typ", "permanent/zettelkasten.typ",])
    );
    assert_eq!(
        index.notes_by_tag("rust").expect("query"),
        paths(&["permanent/note-system.typ"])
    );
    assert_eq!(index.notes_by_tag("absent").expect("query"), no_paths());
}

#[test]
fn backlinks_resolve_target_ids_to_distinct_source_paths() {
    let (_dir, index) = fixture_index();
    assert_eq!(
        index.backlinks(&id("zettelkasten")).expect("query"),
        paths(&[
            "permanent/atomic-notes.typ",
            "permanent/link-traps.typ",
            "permanent/luhmann.typ",
            "permanent/note-system.typ",
            "time/2026-07-21.typ",
        ])
    );
    assert_eq!(
        index.backlinks(&id("luhmann")).expect("query"),
        paths(&["permanent/smart-notes.typ", "permanent/zettelkasten.typ"])
    );
    assert_eq!(
        index.backlinks(&id("no-such-id")).expect("query"),
        no_paths()
    );
}

#[test]
fn repeated_links_from_one_note_yield_one_backlink_row() {
    let (_dir, mut index) = temp_index();
    let notes = vec![
        note(
            "permanent/target.typ",
            NoteCategory::Permanent,
            Some("target"),
            Some(NoteType::Concept),
            &[],
            &[],
        ),
        note(
            "permanent/insistant.typ",
            NoteCategory::Permanent,
            Some("insistant"),
            Some(NoteType::Idea),
            &[],
            &["target", "target"],
        ),
    ];
    index.rebuild(&notes).expect("rebuild");
    assert_eq!(
        index.backlinks(&id("target")).expect("query"),
        paths(&["permanent/insistant.typ"])
    );
}

#[test]
fn duplicate_ids_are_stored_not_rejected() {
    let (_dir, mut index) = temp_index();
    let notes = vec![
        note(
            "permanent/a.typ",
            NoteCategory::Permanent,
            Some("twin"),
            Some(NoteType::Idea),
            &[],
            &[],
        ),
        note(
            "permanent/b.typ",
            NoteCategory::Permanent,
            Some("twin"),
            Some(NoteType::Idea),
            &[],
            &[],
        ),
        note(
            "permanent/c.typ",
            NoteCategory::Permanent,
            Some("c"),
            Some(NoteType::Idea),
            &[],
            &["twin"],
        ),
    ];
    index.rebuild(&notes).expect("rebuild");
    assert_eq!(
        index.notes_by_type(&NoteType::Idea).expect("query").len(),
        3
    );
    // a link to a duplicated id resolves — it is debt, not a dangling link
    assert_eq!(index.dangling_links().expect("query"), no_dangling());
    assert_eq!(
        index.backlinks(&id("twin")).expect("query"),
        paths(&["permanent/c.typ"])
    );
}

#[test]
fn dangling_links_surface_the_planted_fixture() {
    let (_dir, index) = fixture_index();
    assert_eq!(
        index.dangling_links().expect("query"),
        vec![DanglingLink {
            source: PathBuf::from("permanent/atomic-notes.typ"),
            target: id("evergreen-notes"),
        }]
    );
}

#[test]
fn queries_report_sqlite_failures_instead_of_panicking() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    let index = Index::open(&db).expect("open");
    {
        let conn = rusqlite::Connection::open(&db).expect("raw open");
        conn.execute_batch("DROP TABLE links;")
            .expect("drop links table");
    }
    // the first call still holds the old schema, so it prepares fine and dies
    // at step; only once SQLite reloads does prepare itself fail
    assert!(matches!(index.dangling_links(), Err(IndexError::Sqlite(_))));
    assert!(matches!(index.dangling_links(), Err(IndexError::Sqlite(_))));
    assert!(matches!(
        index.backlinks(&id("zettelkasten")),
        Err(IndexError::Sqlite(_))
    ));
}

#[test]
fn queries_report_non_text_columns_instead_of_panicking() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    let index = Index::open(&db).expect("open");
    let raw = rusqlite::Connection::open(&db).expect("raw open");
    // TEXT affinity silently coerces numbers to text but never blobs, so a blob
    // is the only way to get a non-string past a TEXT column
    raw.execute_batch(
        "PRAGMA foreign_keys = off;
         INSERT INTO links (source_path, target_id) VALUES (x'00', 'nowhere');",
    )
    .expect("plant a blob source_path");
    assert!(matches!(index.dangling_links(), Err(IndexError::Sqlite(_))));

    raw.execute_batch(
        "DELETE FROM links;
         INSERT INTO links (source_path, target_id) VALUES ('a.typ', x'00');",
    )
    .expect("plant a blob target_id");
    assert!(matches!(index.dangling_links(), Err(IndexError::Sqlite(_))));

    raw.execute_batch(
        "INSERT INTO notes (path, category) VALUES (x'00', 'permanent');",
    )
    .expect("plant a blob path");
    assert!(matches!(
        index.notes_by_category(&NoteCategory::Permanent),
        Err(IndexError::Sqlite(_))
    ));
}

#[test]
fn typeless_notes_exclude_captures_but_include_missing_meta() {
    let (_dir, index) = fixture_index();
    assert_eq!(
        index.typeless_notes().expect("query"),
        paths(&["permanent/missing-meta.typ", "permanent/missing-type.typ"])
    );
}

// ---------------------------------------------------------------- storage fidelity

#[test]
fn scalar_meta_fields_and_anomalies_are_stored_faithfully() {
    let dir = tempdir();
    let db = dir.path().join("index.sqlite");
    let mut index = Index::open(&db).expect("open");
    let mut notes = scan_fixture();
    notes.push(anomalous_note());
    index.rebuild(&notes).expect("rebuild");
    drop(index);

    let conn = rusqlite::Connection::open(&db).expect("raw open");
    let row: (String, String, String, String) = conn
        .query_row(
            "SELECT id, type, created, origin FROM notes
             WHERE path = 'permanent/atomic-notes.typ'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read atomic-notes row");
    assert_eq!(
        row,
        (
            "atomic-notes".to_string(),
            "claim".to_string(),
            "2026-07-22".to_string(),
            "smart-notes".to_string(),
        )
    );

    let missing: (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT id, type FROM notes WHERE path = 'permanent/missing-meta.typ'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read missing-meta row");
    assert_eq!(missing, (None, None));

    assert_eq!(
        anomaly_rows(&conn, "permanent/duplicate-meta.typ"),
        vec![("duplicate-meta".to_string(), None, None)]
    );
    assert_eq!(
        anomaly_rows(&conn, "permanent/anomalies.typ"),
        vec![
            (
                "invalid-created".to_string(),
                None,
                Some("hier".to_string())
            ),
            (
                "malformed-field".to_string(),
                Some("tags".to_string()),
                Some("(\"oops\"".to_string()),
            ),
        ]
    );
}

// ---------------------------------------------------------------- helpers

fn fixture_vault() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
}

fn scan_fixture() -> Vec<Note> {
    scan_vault(&fixture_vault()).expect("fixture vault scans")
}

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("create tempdir")
}

fn temp_index() -> (tempfile::TempDir, Index) {
    let dir = tempdir();
    let index = Index::open(&dir.path().join("index.sqlite"))
        .expect("open fresh index");
    (dir, index)
}

fn fixture_index() -> (tempfile::TempDir, Index) {
    let (dir, mut index) = temp_index();
    index
        .rebuild(&scan_fixture())
        .expect("rebuild from fixture vault");
    (dir, index)
}

fn snapshot(index: &Index) -> Vec<Vec<PathBuf>> {
    vec![
        index
            .notes_by_category(&NoteCategory::Permanent)
            .expect("by category"),
        index.notes_by_type(&NoteType::Daily).expect("by type"),
        index.notes_by_tag("method").expect("by tag"),
        index.backlinks(&id("zettelkasten")).expect("backlinks"),
        index.typeless_notes().expect("typeless"),
        index
            .dangling_links()
            .expect("dangling")
            .into_iter()
            .map(|d| d.source)
            .collect(),
    ]
}

fn note(
    path: &str,
    category: NoteCategory,
    note_id: Option<&str>,
    note_type: Option<NoteType>,
    tags: &[&str],
    links: &[&str],
) -> Note {
    Note {
        path: PathBuf::from(path),
        category,
        meta: MetaStatus::Present(Meta {
            id: note_id.map(|i| NoteId(i.to_string())),
            note_type,
            created: None,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            origin: None,
            anomalies: vec![],
        }),
        links: links
            .iter()
            .map(|t| Link {
                target: NoteId(t.to_string()),
            })
            .collect(),
    }
}

fn anomalous_note() -> Note {
    Note {
        path: PathBuf::from("permanent/anomalies.typ"),
        category: NoteCategory::Permanent,
        meta: MetaStatus::Present(Meta {
            id: Some(NoteId("anomalies".to_string())),
            note_type: None,
            created: None,
            tags: vec![],
            origin: None,
            anomalies: vec![
                MetaAnomaly::InvalidCreated("hier".to_string()),
                MetaAnomaly::MalformedField(
                    "tags".to_string(),
                    "(\"oops\"".to_string(),
                ),
            ],
        }),
        links: vec![],
    }
}

fn anomaly_rows(
    conn: &rusqlite::Connection,
    note_path: &str,
) -> Vec<(String, Option<String>, Option<String>)> {
    let mut statement = conn
        .prepare("SELECT kind, field, raw FROM anomalies WHERE note_path = ?1 ORDER BY kind")
        .expect("prepare anomaly query");
    let rows = statement
        .query_map([note_path], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("run anomaly query");
    rows.collect::<Result<Vec<_>, _>>()
        .expect("collect anomaly rows")
}

fn paths(relative: &[&str]) -> Vec<PathBuf> {
    relative.iter().map(PathBuf::from).collect()
}

// `vec![]` alone cannot infer its element type through assert_eq!
fn no_paths() -> Vec<PathBuf> {
    vec![]
}

fn no_dangling() -> Vec<DanglingLink> {
    vec![]
}

fn id(raw: &str) -> NoteId {
    NoteId(raw.to_string())
}

fn set_readonly(path: &Path, readonly: bool) {
    let mut permissions =
        std::fs::metadata(path).expect("stat path").permissions();
    permissions.set_readonly(readonly);
    std::fs::set_permissions(path, permissions).expect("set permissions");
}
