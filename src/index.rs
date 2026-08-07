use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::domain::{
    Meta, MetaAnomaly, MetaStatus, Note, NoteCategory, NoteId, NoteType,
};
use crate::parse;
use rusqlite::{Connection, Row, Transaction};

pub const SCHEMA_VERSION: i32 = 3;
const FOREIGN_KEYS: &str = "PRAGMA foreign_keys = on;";
const SCHEMA: &str = r#"
CREATE TABLE notes (
    path     TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    id       TEXT,
    type     TEXT,
    created  TEXT,
    origin   TEXT,
    title    TEXT,
    summarized INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE links (
    source_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    target_id   TEXT NOT NULL
);
CREATE TABLE tags (
    note_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    tag       TEXT NOT NULL
);
CREATE TABLE anomalies (
    note_path TEXT NOT NULL REFERENCES notes(path) ON DELETE CASCADE,
    kind      TEXT NOT NULL,
    field     TEXT,
    raw       TEXT
);
"#;

#[derive(Debug)]
pub enum IndexError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
}

impl From<std::io::Error> for IndexError {
    fn from(error: std::io::Error) -> IndexError {
        IndexError::Io(error)
    }
}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> IndexError {
        IndexError::Sqlite(error)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct DanglingLink {
    pub source: PathBuf,
    pub target: NoteId,
}

/// A note linking *to* the one being read. The source's own id is carried
/// along because the footer labels backlinks by id, and an id-less source
/// (visible debt, still a real link) has to fall back to its filename.
#[derive(Debug, PartialEq, Eq)]
pub struct Backlink {
    pub source: PathBuf,
    pub id: Option<String>,
}

pub struct Index {
    connection: Connection,
}

impl Index {
    pub fn open(db_path: &Path) -> Result<Index, IndexError> {
        let connection = open_connection(db_path)?;
        if schema_version(&connection) == Some(SCHEMA_VERSION) {
            return Ok(Index { connection });
        }
        drop(connection);
        discard(db_path)?;

        let connection = open_connection(faults::reopen_path(db_path))?;
        create_schema(&connection)?;
        Ok(Index { connection })
    }

    pub fn rebuild(&mut self, notes: &[Note]) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        transaction.execute_batch(concat!(
            "DELETE FROM anomalies;",
            "DELETE FROM tags;",
            "DELETE FROM links;",
            "DELETE FROM notes;",
        ))?;
        for note in notes {
            insert_note(&transaction, note)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn update_note(&mut self, note: &Note) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        delete_note(&transaction, &note.path)?;
        insert_note(&transaction, note)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn remove_note(&mut self, path: &Path) -> Result<(), IndexError> {
        let transaction = self.connection.transaction()?;
        delete_note(&transaction, path)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn notes_by_category(
        &self,
        category: &NoteCategory,
    ) -> Result<Vec<PathBuf>, IndexError> {
        query_paths(
            &self.connection,
            "SELECT path FROM notes WHERE category = ?1 ORDER BY path",
            [category.as_dir()],
        )
    }

    pub fn notes_by_type(
        &self,
        note_type: &NoteType,
    ) -> Result<Vec<PathBuf>, IndexError> {
        query_paths(
            &self.connection,
            "SELECT path FROM notes WHERE type = ?1 ORDER BY path",
            [note_type.as_name()],
        )
    }

    pub fn notes_by_tag(&self, tag: &str) -> Result<Vec<PathBuf>, IndexError> {
        query_paths(
            &self.connection,
            "SELECT note_path FROM tags WHERE tag = ?1 ORDER BY note_path",
            [tag],
        )
    }

    pub fn backlinks(
        &self,
        target: &NoteId,
    ) -> Result<Vec<Backlink>, IndexError> {
        query_rows(
            &self.connection,
            concat!(
                "SELECT DISTINCT notes.path, notes.id ",
                "FROM links ",
                "JOIN notes ON notes.path = links.source_path ",
                "WHERE links.target_id = ?1 ",
                "ORDER BY notes.path"
            ),
            [target.0.as_str()],
            |row| {
                Ok(Backlink {
                    source: PathBuf::from(row.get::<_, String>(0)?),
                    id: row.get::<_, Option<String>>(1)?,
                })
            },
        )
    }

    /// Every note the link picker can offer, as (id, title). An id-less note
    /// cannot be a link target, so it is not a completion — it is open-loops
    /// debt instead. Duplicate ids are not deduplicated: a collision is an
    /// error to see, not to hide (adr/2026-07-id-collision-is-an-error.md).
    pub fn completions(
        &self,
    ) -> Result<Vec<(String, Option<String>)>, IndexError> {
        query_rows(
            &self.connection,
            "SELECT id, title FROM notes WHERE id IS NOT NULL ORDER BY id",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
    }

    pub fn typeless_notes(&self) -> Result<Vec<PathBuf>, IndexError> {
        query_paths(
            &self.connection,
            concat!(
                "SELECT path ",
                "FROM notes ",
                "WHERE type IS NULL AND category != ?1 ",
                "ORDER BY path"
            ),
            [NoteCategory::Capture.as_dir()],
        )
    }

    /// Captures whose `== Summary` section is still empty — the third kind
    /// of open-loops debt, beside typeless notes and dangling links
    /// (adr/2026-08-summarized-nonempty-summary-section.md). Only captures
    /// carry this loop: a permanent note owes no summary to anyone.
    pub fn unsummarized_captures(&self) -> Result<Vec<PathBuf>, IndexError> {
        query_paths(
            &self.connection,
            concat!(
                "SELECT path ",
                "FROM notes ",
                "WHERE category = ?1 AND summarized = 0 ",
                "ORDER BY path"
            ),
            [NoteCategory::Capture.as_dir()],
        )
    }

    pub fn path_for_id(
        &self,
        id: &NoteId,
    ) -> Result<Option<PathBuf>, IndexError> {
        query_first(
            &self.connection,
            // ORDER BY path: duplicate ids are stored, not rejected, so the
            // answer must at least be deterministic
            "SELECT path FROM notes WHERE id = ?1 ORDER BY path LIMIT 1",
            [id.0.as_str()],
        )
    }

    /// The nearest daily note strictly before `day`, or `None` at the edge.
    /// Daily ids sort lexicographically = chronologically (`YYYY-MM-DD`), so
    /// the comparison *is* the gap resolution — `day` itself need not exist.
    pub fn daily_before(
        &self,
        day: &NoteId,
    ) -> Result<Option<PathBuf>, IndexError> {
        query_first(
            &self.connection,
            // 'daily' stays a literal: a second parameter would mint a new
            // generic instantiation of the query helpers (coverage cost)
            concat!(
                "SELECT path FROM notes ",
                "WHERE type = 'daily' AND id < ?1 ",
                "ORDER BY id DESC LIMIT 1"
            ),
            [day.0.as_str()],
        )
    }

    /// The nearest daily note strictly after `day`; see `daily_before`.
    pub fn daily_after(
        &self,
        day: &NoteId,
    ) -> Result<Option<PathBuf>, IndexError> {
        query_first(
            &self.connection,
            concat!(
                "SELECT path FROM notes ",
                "WHERE type = 'daily' AND id > ?1 ",
                "ORDER BY id ASC LIMIT 1"
            ),
            [day.0.as_str()],
        )
    }

    /// Every time note the rail can show, as `(id, type)`, ordered by id
    /// for determinism (the rail re-sorts by scale hierarchy anyway).
    /// Typeless or id-less time notes are open-loops debt, not rail rows
    /// (adr/2026-07-rail-continuous-newest-first.md).
    pub fn time_notes(&self) -> Result<Vec<(String, NoteType)>, IndexError> {
        query_rows(
            &self.connection,
            concat!(
                "SELECT id, type FROM notes ",
                "WHERE category = 'time' ",
                "AND id IS NOT NULL AND type IS NOT NULL ",
                "ORDER BY id"
            ),
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    NoteType::from_name(&row.get::<_, String>(1)?),
                ))
            },
        )
    }

    /// The capture and generated notes created on `date` (ISO `YYYY-MM-DD`,
    /// the column's stored form) — the "captured today" block under a day
    /// note. Returns the file stem as the display label. The category is
    /// derived from the stored path's leading directory rather than read
    /// back from the column: the WHERE clause already guarantees the
    /// column's value, so re-reading it would only add an untestable
    /// decode branch.
    pub fn captured_on(
        &self,
        date: &str,
    ) -> Result<Vec<(String, NoteCategory)>, IndexError> {
        let paths = query_paths(
            &self.connection,
            concat!(
                "SELECT path FROM notes ",
                "WHERE category IN ('capture', 'generated') ",
                "AND created = ?1 ",
                "ORDER BY path"
            ),
            [date],
        )?;
        Ok(paths
            .iter()
            .map(|path| {
                let category = if path.starts_with("generated") {
                    NoteCategory::Generated
                } else {
                    NoteCategory::Capture
                };
                (crate::domain::stem_of(path), category)
            })
            .collect())
    }

    pub fn dangling_links(&self) -> Result<Vec<DanglingLink>, IndexError> {
        query_rows(
            &self.connection,
            concat!(
                "SELECT DISTINCT links.source_path, links.target_id ",
                "FROM links ",
                "LEFT JOIN notes ON notes.id = links.target_id ",
                "WHERE notes.id IS NULL ",
                "ORDER BY links.source_path, links.target_id"
            ),
            [],
            |row| {
                Ok(DanglingLink {
                    source: PathBuf::from(row.get::<_, String>(0)?),
                    target: NoteId(row.get::<_, String>(1)?),
                })
            },
        )
    }
}

pub fn scan_vault(root: &Path) -> Result<Vec<Note>, IndexError> {
    let mut notes = Vec::new();
    for entry in faults::vault_entries(std::fs::read_dir(root)?) {
        let entry = entry?;
        let Some(category) =
            entry.file_name().to_str().and_then(NoteCategory::from_dir)
        else {
            continue;
        };
        for file in faults::category_entries(std::fs::read_dir(entry.path())?)
        {
            let path = file?;
            let name = path.file_name();
            if Path::new(&name).extension() == Some(OsStr::new("typ")) {
                let parsed_note =
                    parse::parse_note(&std::fs::read_to_string(path.path())?);

                notes.push(Note {
                    path: Path::new(category.as_dir()).join(&name),
                    category,
                    meta: parsed_note.meta,
                    title: parsed_note.title,
                    links: parsed_note.links,
                    summarized: parsed_note.summarized,
                })
            }
        }
    }
    notes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(notes)
}

fn schema_version(connection: &Connection) -> Option<i32> {
    // a file that is not a readable database is not an error to report but a
    // reason to rebuild — both answers mean "this index is unusable"
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .ok()
}

fn open_connection(db_path: &Path) -> Result<Connection, rusqlite::Error> {
    let connection = Connection::open(db_path)?;
    connection.execute_batch(faults::foreign_keys_sql())?;
    Ok(connection)
}

/// Delete a stale index file.
///
/// Testing `exists()` first would leave a window in which another process
/// removes the file and we report its absence as a failure, so the missing
/// case is handled rather than pre-checked.
fn discard(db_path: &Path) -> Result<(), IndexError> {
    match std::fs::remove_file(db_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(IndexError::Io(error)),
    }
}

/// Create the tables, then stamp the version.
///
/// The order matters: a version written first would label a database whose
/// tables failed to appear as a complete index, and every later `open` would
/// trust it.
fn create_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(faults::schema_sql())?;
    // PRAGMA cannot take bound parameters; SCHEMA_VERSION is a compile-time
    // constant, never user input
    connection
        .execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    Ok(())
}

fn delete_note(
    transaction: &Transaction,
    path: &Path,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "DELETE FROM notes WHERE path = ?1",
        rusqlite::params![path.to_string_lossy()],
    )?;
    Ok(())
}

fn insert_note(
    transaction: &Transaction,
    note: &Note,
) -> Result<(), rusqlite::Error> {
    let meta = match &note.meta {
        MetaStatus::Present(meta) => meta,
        MetaStatus::Missing => &Meta::default(),
    };
    transaction.execute(
        concat!(
            "INSERT INTO notes ",
            "(path, category, id, type, created, origin, title, summarized)",
            "VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        ),
        rusqlite::params![
            note.path.to_string_lossy(),
            note.category.as_dir(),
            meta.id.as_ref().map(|id| id.0.as_str()),
            meta.note_type.as_ref().map(NoteType::as_name),
            meta.created,
            meta.origin.as_deref(),
            note.title.as_deref(),
            note.summarized,
        ],
    )?;
    for tag in &meta.tags {
        transaction.execute(
            concat!("INSERT INTO tags (note_path, tag)", "VALUES (?1, ?2)"),
            rusqlite::params![note.path.to_string_lossy(), tag],
        )?;
    }
    for link in &note.links {
        transaction.execute(
            concat!(
                "INSERT INTO links (source_path, target_id)",
                "VALUES (?1, ?2)"
            ),
            rusqlite::params![
                note.path.to_string_lossy(),
                link.target.0.as_str()
            ],
        )?;
    }
    for anomaly in &meta.anomalies {
        let (kind, field, raw) = match anomaly {
            MetaAnomaly::DuplicateMeta => ("duplicate-meta", None, None),
            MetaAnomaly::InvalidCreated(raw) => {
                ("invalid-created", None, Some(raw.as_str()))
            }
            MetaAnomaly::MalformedField(field, raw) => {
                ("malformed-field", Some(field.as_str()), Some(raw.as_str()))
            }
        };
        transaction.execute(
            concat!(
                "INSERT INTO anomalies (note_path, kind, field, raw)",
                "VALUES (?1, ?2, ?3, ?4)"
            ),
            rusqlite::params![note.path.to_string_lossy(), kind, field, raw],
        )?;
    }
    Ok(())
}

fn query_first(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Option<PathBuf>, IndexError> {
    Ok(query_paths(connection, sql, params)?.into_iter().next())
}

fn query_paths(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<PathBuf>, IndexError> {
    query_rows(connection, sql, params, |row| {
        Ok(PathBuf::from(row.get::<_, String>(0)?))
    })
}

fn query_rows<T, F>(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
    to_row: F,
) -> Result<Vec<T>, IndexError>
where
    F: FnMut(&Row<'_>) -> rusqlite::Result<T>,
{
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params, to_row)?;
    let collected = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(collected)
}

/// Fault injection for the error paths that cannot be reached through the
/// filesystem or SQLite itself.
///
/// Outside `cfg(test)` every function here is the identity, so the shipped code
/// path is the one the tests exercise. The module is excluded from coverage: it
/// is scaffolding, and measuring it would only ever measure the arm that the
/// current build compiled.
#[cfg_attr(coverage_nightly, coverage(off))]
mod faults {
    use std::fs::{DirEntry, ReadDir};
    use std::io;
    use std::path::Path;

    #[cfg(not(test))]
    pub(super) fn reopen_path(db_path: &Path) -> &Path {
        db_path
    }

    #[cfg(not(test))]
    pub(super) fn foreign_keys_sql() -> &'static str {
        super::FOREIGN_KEYS
    }

    #[cfg(not(test))]
    pub(super) fn schema_sql() -> &'static str {
        super::SCHEMA
    }

    #[cfg(not(test))]
    pub(super) fn vault_entries(
        entries: ReadDir,
    ) -> impl Iterator<Item = io::Result<DirEntry>> {
        entries
    }

    #[cfg(not(test))]
    pub(super) fn category_entries(
        entries: ReadDir,
    ) -> impl Iterator<Item = io::Result<DirEntry>> {
        entries
    }

    #[cfg(test)]
    pub(super) use armed::*;

    #[cfg(test)]
    mod armed {
        use super::*;
        use std::cell::Cell;

        /// Never a valid SQLite database — it is a directory.
        const UNOPENABLE: &str = "/";
        const NOT_SQL: &str = "this is not sql;";

        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(in crate::index) enum Fault {
            Reopen,
            ForeignKeys,
            Schema,
            VaultEntries,
            CategoryEntries,
        }

        thread_local! {
            static ARMED: Cell<Option<Fault>> = const { Cell::new(None) };
        }

        /// Arms `fault` until the returned guard drops, so a panicking test
        /// cannot leak the fault into the next test on the same thread.
        pub(in crate::index) fn arm(fault: Fault) -> Guard {
            ARMED.with(|armed| armed.set(Some(fault)));
            Guard
        }

        pub(in crate::index) struct Guard;

        impl Drop for Guard {
            fn drop(&mut self) {
                ARMED.with(|armed| armed.set(None));
            }
        }

        fn is_armed(fault: Fault) -> bool {
            ARMED.with(|armed| armed.get()) == Some(fault)
        }

        pub(in crate::index) fn reopen_path(db_path: &Path) -> &Path {
            if is_armed(Fault::Reopen) {
                Path::new(UNOPENABLE)
            } else {
                db_path
            }
        }

        pub(in crate::index) fn foreign_keys_sql() -> &'static str {
            if is_armed(Fault::ForeignKeys) {
                NOT_SQL
            } else {
                crate::index::FOREIGN_KEYS
            }
        }

        pub(in crate::index) fn schema_sql() -> &'static str {
            if is_armed(Fault::Schema) {
                NOT_SQL
            } else {
                crate::index::SCHEMA
            }
        }

        pub(in crate::index) fn vault_entries(
            entries: ReadDir,
        ) -> Box<dyn Iterator<Item = io::Result<DirEntry>>> {
            inject(Fault::VaultEntries, entries)
        }

        pub(in crate::index) fn category_entries(
            entries: ReadDir,
        ) -> Box<dyn Iterator<Item = io::Result<DirEntry>>> {
            inject(Fault::CategoryEntries, entries)
        }

        /// `readdir` failing mid-walk is unreachable on a local filesystem, so
        /// the failing entry is prepended instead.
        fn inject(
            fault: Fault,
            entries: ReadDir,
        ) -> Box<dyn Iterator<Item = io::Result<DirEntry>>> {
            if is_armed(fault) {
                Box::new(
                    std::iter::once(Err(io::Error::from(
                        io::ErrorKind::PermissionDenied,
                    )))
                    .chain(entries),
                )
            } else {
                Box::new(entries)
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    //! Fault injection. These live beside the code rather than in
    //! `tests/integration/` because forcing SQLite to fail needs the private
    //! connection: an authorizer refuses one chosen statement at prepare time,
    //! which exercises an error path without faking rusqlite.

    use super::*;
    use crate::domain::Link;
    use faults::Fault;
    use rusqlite::hooks::{
        AuthAction, AuthContext, Authorization, TransactionOperation,
    };

    /// One compilation of this file has to cover it entirely: llvm-cov folds an
    /// instantiation group by taking the highest covered count of a single
    /// copy, never the union of both. This test is what makes the unit-test
    /// copy self-sufficient for the whole happy path.
    #[test]
    fn scanning_and_rebuilding_the_fixture_vault_answers_queries() {
        let (_dir, mut index) = temp_index();
        let mut notes = scan_vault(&fixture_vault()).expect("scan fixture");
        notes.push(rich_note());
        index.rebuild(&notes).expect("rebuild");
        assert!(
            !index
                .notes_by_category(&NoteCategory::Permanent)
                .expect("query")
                .is_empty()
        );
        assert!(!index.dangling_links().expect("query").is_empty());
    }

    #[test]
    fn time_notes_lists_ids_and_types_for_complete_time_notes_only() {
        let (dir, mut index) = temp_index();
        let notes = scan_vault(&fixture_vault()).expect("scan fixture");
        index.rebuild(&notes).expect("rebuild");
        // debt rows stay invisible: a typeless and an id-less time note
        let raw = Connection::open(dir.path().join("index.sqlite"))
            .expect("raw open");
        raw.execute_batch(concat!(
            "INSERT INTO notes (path, category, id) ",
            "VALUES ('time/stray.typ', 'time', 'stray');",
            "INSERT INTO notes (path, category, type) ",
            "VALUES ('time/anonymous.typ', 'time', 'daily');",
        ))
        .expect("plant debt rows");

        assert_eq!(
            index.time_notes().expect("query"),
            vec![
                ("2026-07-21".to_string(), NoteType::Daily),
                ("2026-07-22".to_string(), NoteType::Daily),
                ("2026-07-23".to_string(), NoteType::Daily),
                ("2026-summer".to_string(), NoteType::Seasonal),
                ("2026-w30".to_string(), NoteType::Weekly),
            ]
        );
    }

    #[test]
    fn time_notes_reports_rows_that_will_not_decode() {
        // one blob per column read, so each `?` in the closure fires
        for plant in [
            "INSERT INTO notes (path, category, id, type)
             VALUES ('time/blob-id.typ', 'time', x'00', 'daily');",
            "INSERT INTO notes (path, category, id, type)
             VALUES ('time/blob-type.typ', 'time', 'ok', x'00');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the blob row");
            assert!(matches!(index.time_notes(), Err(IndexError::Sqlite(_))));
        }
    }

    #[test]
    fn completions_report_rows_that_will_not_decode() {
        // one blob per column read, so each `?` in the closure fires
        for plant in [
            "INSERT INTO notes (path, category, id, title)
             VALUES ('permanent/blob-id.typ', 'permanent', x'00', 'ok');",
            "INSERT INTO notes (path, category, id, title)
             VALUES ('permanent/blob-title.typ', 'permanent', 'ok', x'00');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the blob row");
            assert!(matches!(index.completions(), Err(IndexError::Sqlite(_))));
        }
    }

    #[test]
    fn backlinks_report_rows_that_will_not_decode() {
        for plant in [
            "INSERT INTO notes (path, category, id)
             VALUES (x'00', 'permanent', 'source');
             INSERT INTO links (source_path, target_id)
             VALUES (x'00', 'target');",
            "INSERT INTO notes (path, category, id)
             VALUES ('permanent/blob-id.typ', 'permanent', x'00');
             INSERT INTO links (source_path, target_id)
             VALUES ('permanent/blob-id.typ', 'target');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the blob row");
            assert!(matches!(
                index.backlinks(&NoteId("target".to_string())),
                Err(IndexError::Sqlite(_))
            ));
        }
    }

    #[test]
    fn captured_on_gathers_captures_and_generated_by_creation_date() {
        let (dir, mut index) = temp_index();
        let notes = scan_vault(&fixture_vault()).expect("scan fixture");
        index.rebuild(&notes).expect("rebuild");
        // a permanent note created the same day must stay out
        let raw = Connection::open(dir.path().join("index.sqlite"))
            .expect("raw open");
        raw.execute_batch(concat!(
            "INSERT INTO notes (path, category, created) ",
            "VALUES ('permanent/same-day.typ', 'permanent', '2026-07-23');",
        ))
        .expect("plant the same-day permanent note");

        assert_eq!(
            index.captured_on("2026-07-23").expect("query"),
            vec![
                ("capture-idea-canvas".to_string(), NoteCategory::Capture),
                ("digest-smart-notes".to_string(), NoteCategory::Generated),
            ]
        );
        assert_eq!(index.captured_on("1999-01-01").expect("query"), vec![]);
    }

    #[test]
    fn captured_on_reports_rows_that_will_not_decode() {
        // only the path can arrive undecodable: a blob category would never
        // match the WHERE clause's text comparison in the first place
        let (dir, index) = temp_index();
        let raw = Connection::open(dir.path().join("index.sqlite"))
            .expect("raw open");
        raw.execute_batch(
            "INSERT INTO notes (path, category, created)
             VALUES (x'00', 'capture', '2026-07-23');",
        )
        .expect("plant the blob row");
        assert!(matches!(
            index.captured_on("2026-07-23"),
            Err(IndexError::Sqlite(_))
        ));
    }

    #[test]
    fn scan_vault_reports_filesystem_failures() {
        let dir = tempfile::tempdir().expect("create tempdir");
        assert!(matches!(
            scan_vault(&dir.path().join("no-such-vault")),
            Err(IndexError::Io(_))
        ));

        let permanent = dir.path().join("permanent");
        std::fs::write(&permanent, b"a file, not a directory")
            .expect("write fake category");
        assert!(matches!(scan_vault(dir.path()), Err(IndexError::Io(_))));

        std::fs::remove_file(&permanent).expect("remove fake category");
        std::fs::create_dir(&permanent).expect("create category dir");
        std::fs::write(permanent.join("broken.typ"), [0xff, 0xfe, 0x00])
            .expect("write invalid utf-8");
        assert!(matches!(scan_vault(dir.path()), Err(IndexError::Io(_))));
    }

    #[test]
    fn rebuild_reports_two_notes_claiming_one_path() {
        let (_dir, mut index) = temp_index();
        assert!(matches!(
            index.rebuild(&[rich_note(), rich_note()]),
            Err(IndexError::Sqlite(_))
        ));
    }

    #[test]
    fn query_paths_reports_prepare_and_decode_failures() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db = dir.path().join("index.sqlite");
        let index = Index::open(&db).expect("open index");
        // the parameter type is part of query_rows' instantiation, so this has
        // to match what the public queries pass or it covers a separate copy
        assert!(
            query_paths(
                &index.connection,
                "SELECT nope FROM nope",
                ["unused"]
            )
            .is_err()
        );

        let raw = Connection::open(&db).expect("raw open");
        // TEXT affinity coerces numbers but never blobs, so this will not decode
        raw.execute_batch(
            "INSERT INTO notes (path, category) VALUES (x'00', 'permanent');",
        )
        .expect("plant a blob path");
        assert!(index.notes_by_category(&NoteCategory::Permanent).is_err());
    }

    #[test]
    fn open_reuses_an_index_already_at_the_current_version() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db = dir.path().join("index.sqlite");
        drop(Index::open(&db).expect("create index"));
        assert!(Index::open(&db).is_ok());
    }

    #[test]
    fn open_reports_a_stale_index_it_cannot_delete() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let db = dir.path().join("index.sqlite");
        std::fs::write(&db, b"this is not a sqlite database")
            .expect("write garbage");
        // unlinking needs write permission on the directory, not the file
        set_readonly(dir.path(), true);
        let result = Index::open(&db);
        // restore first: a read-only dir would also defeat the tempdir cleanup
        set_readonly(dir.path(), false);
        assert!(matches!(result, Err(IndexError::Io(_))));
    }

    #[test]
    fn rebuild_reports_a_refused_delete() {
        let (_dir, mut index) = temp_index();
        refuse(&index, |action| matches!(action, AuthAction::Delete { .. }));
        assert!(matches!(index.rebuild(&[]), Err(IndexError::Sqlite(_))));
    }

    #[test]
    fn open_reports_a_refused_foreign_keys_pragma() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let _armed = faults::arm(Fault::ForeignKeys);
        assert!(matches!(
            Index::open(&dir.path().join("index.sqlite")),
            Err(IndexError::Sqlite(_))
        ));
    }

    #[test]
    fn open_reports_a_second_connection_that_will_not_open() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let _armed = faults::arm(Fault::Reopen);
        assert!(matches!(
            Index::open(&dir.path().join("index.sqlite")),
            Err(IndexError::Sqlite(_))
        ));
    }

    #[test]
    fn open_reports_a_schema_that_will_not_create() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let _armed = faults::arm(Fault::Schema);
        assert!(matches!(
            Index::open(&dir.path().join("index.sqlite")),
            Err(IndexError::Sqlite(_))
        ));
    }

    #[test]
    fn discarding_an_absent_index_is_not_a_failure() {
        let dir = tempfile::tempdir().expect("create tempdir");
        // SQLite creates the file as soon as a connection opens, so `open`
        // itself never reaches this arm — only a caller that never opened does
        assert!(discard(&dir.path().join("never-existed.sqlite")).is_ok());
    }

    #[test]
    fn create_schema_reports_a_refused_version_stamp() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let connection = Connection::open(dir.path().join("index.sqlite"))
            .expect("open raw connection");
        // the tables are still allowed, so only the version stamp can fail
        refuse_on(&connection, |action| {
            matches!(action, AuthAction::Pragma { .. })
        });
        assert!(create_schema(&connection).is_err());
    }

    #[test]
    fn scan_vault_reports_an_unreadable_vault_entry() {
        let _armed = faults::arm(Fault::VaultEntries);
        assert!(matches!(
            scan_vault(&fixture_vault()),
            Err(IndexError::Io(_))
        ));
    }

    #[test]
    fn scan_vault_reports_an_unreadable_category_entry() {
        let _armed = faults::arm(Fault::CategoryEntries);
        assert!(matches!(
            scan_vault(&fixture_vault()),
            Err(IndexError::Io(_))
        ));
    }

    #[test]
    fn rebuild_reports_a_refused_begin() {
        let (_dir, mut index) = temp_index();
        refuse(&index, |action| {
            matches!(
                action,
                AuthAction::Transaction {
                    operation: TransactionOperation::Begin
                }
            )
        });
        assert!(matches!(index.rebuild(&[]), Err(IndexError::Sqlite(_))));
    }

    #[test]
    fn rebuild_reports_a_refused_commit() {
        let (_dir, mut index) = temp_index();
        // rusqlite maps only BEGIN/RELEASE/ROLLBACK by name, so COMMIT arrives
        // as Unknown — matching it is how we single out the commit
        refuse(&index, |action| {
            matches!(
                action,
                AuthAction::Transaction {
                    operation: TransactionOperation::Unknown
                }
            )
        });
        assert!(matches!(index.rebuild(&[]), Err(IndexError::Sqlite(_))));
    }

    #[test]
    fn rebuild_reports_a_refused_child_row() {
        // one table per child loop in insert_note, each refused on its own
        for table in ["tags", "links", "anomalies"] {
            let (_dir, mut index) = temp_index();
            refuse(
                &index,
                move |action| matches!(action, AuthAction::Insert { table_name } if *table_name == table),
            );
            assert!(
                matches!(
                    index.rebuild(&[rich_note()]),
                    Err(IndexError::Sqlite(_))
                ),
                "refusing INSERT INTO {table} must surface as an error"
            );
        }
    }

    #[test]
    fn query_paths_reports_a_parameter_count_mismatch() {
        let (_dir, index) = temp_index();
        assert!(matches!(
            query_paths(
                &index.connection,
                "SELECT path FROM notes",
                ["one parameter too many"]
            ),
            Err(IndexError::Sqlite(_))
        ));
    }

    #[test]
    fn updating_a_note_replaces_every_row_it_owned() {
        let (_dir, mut index) = temp_index();
        index.rebuild(&[rich_note()]).expect("seed the index");

        let mut plain = rich_note();
        plain.links.clear();
        plain.meta = MetaStatus::Missing;
        index.update_note(&plain).expect("update the note");

        // the tags, links and anomalies rows are only gone if the delete
        // cascaded — nothing here deletes them by name
        assert_eq!(
            index.notes_by_tag("method").expect("by tag"),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            index
                .backlinks(&NoteId("elsewhere".to_string()))
                .expect("backlinks"),
            Vec::<Backlink>::new()
        );
        assert_eq!(
            index
                .notes_by_category(&NoteCategory::Permanent)
                .expect("by category"),
            vec![PathBuf::from("permanent/rich.typ")]
        );
    }

    #[test]
    fn removing_a_note_forgets_it_entirely() {
        let (_dir, mut index) = temp_index();
        index.rebuild(&[rich_note()]).expect("seed the index");

        index
            .remove_note(Path::new("permanent/rich.typ"))
            .expect("remove the note");

        assert_eq!(
            index
                .notes_by_category(&NoteCategory::Permanent)
                .expect("by category"),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            index.notes_by_tag("method").expect("by tag"),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn update_note_reports_every_refused_step() {
        // one arm per `?` in update_note
        let steps = [Step::Begin, Step::Delete, Step::Insert, Step::Commit];
        for step in steps {
            let (_dir, mut index) = temp_index();
            refuse_step(&index, step);
            assert!(
                matches!(
                    index.update_note(&rich_note()),
                    Err(IndexError::Sqlite(_))
                ),
                "a refused {step:?} must be reported, not panicked on"
            );
        }
    }

    #[test]
    fn remove_note_reports_every_refused_step() {
        // remove_note has no insert, so three `?` rather than four
        for step in [Step::Begin, Step::Delete, Step::Commit] {
            let (_dir, mut index) = temp_index();
            refuse_step(&index, step);
            assert!(
                matches!(
                    index.remove_note(Path::new("permanent/rich.typ")),
                    Err(IndexError::Sqlite(_))
                ),
                "a refused {step:?} must be reported, not panicked on"
            );
        }
    }

    fn temp_index() -> (tempfile::TempDir, Index) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let index = Index::open(&dir.path().join("index.sqlite"))
            .expect("open fresh index");
        (dir, index)
    }

    fn fixture_vault() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
    }

    /// The statements `update_note` and `remove_note` issue, one per `?`.
    #[derive(Clone, Copy, Debug)]
    enum Step {
        Begin,
        Delete,
        Insert,
        Commit,
    }

    fn refuse_step(index: &Index, step: Step) {
        refuse(index, move |action| match step {
            Step::Begin => matches!(
                action,
                AuthAction::Transaction {
                    operation: TransactionOperation::Begin
                }
            ),
            Step::Delete => {
                matches!(action, AuthAction::Delete { table_name } if *table_name == "notes")
            }
            Step::Insert => {
                matches!(action, AuthAction::Insert { table_name } if *table_name == "notes")
            }
            // rusqlite maps only BEGIN/RELEASE/ROLLBACK by name, so COMMIT
            // arrives as Unknown
            Step::Commit => matches!(
                action,
                AuthAction::Transaction {
                    operation: TransactionOperation::Unknown
                }
            ),
        });
    }

    fn refuse<F>(index: &Index, is_refused: F)
    where
        F: for<'r> FnMut(&AuthAction<'r>) -> bool + Send + 'static,
    {
        refuse_on(&index.connection, is_refused);
    }

    fn refuse_on<F>(connection: &Connection, mut is_refused: F)
    where
        F: for<'r> FnMut(&AuthAction<'r>) -> bool + Send + 'static,
    {
        connection
            .authorizer(Some(move |context: AuthContext<'_>| {
                if is_refused(&context.action) {
                    Authorization::Deny
                } else {
                    Authorization::Allow
                }
            }))
            .expect("install authorizer");
    }

    fn set_readonly(path: &Path, readonly: bool) {
        let mut permissions =
            std::fs::metadata(path).expect("stat path").permissions();
        permissions.set_readonly(readonly);
        std::fs::set_permissions(path, permissions).expect("set permissions");
    }

    /// A note that reaches every child loop and every anomaly arm of
    /// `insert_note`.
    fn rich_note() -> Note {
        Note {
            path: PathBuf::from("permanent/rich.typ"),
            category: NoteCategory::Permanent,
            meta: MetaStatus::Present(Meta {
                id: Some(NoteId("rich".to_string())),
                note_type: Some(NoteType::Idea),
                created: None,
                tags: vec!["method".to_string()],
                origin: None,
                anomalies: vec![
                    MetaAnomaly::DuplicateMeta,
                    MetaAnomaly::InvalidCreated("hier".to_string()),
                    MetaAnomaly::MalformedField(
                        "tags".to_string(),
                        "(\"oops\"".to_string(),
                    ),
                ],
            }),
            title: Some("A rich note".to_string()),
            links: vec![Link {
                target: NoteId("elsewhere".to_string()),
            }],
            summarized: true,
        }
    }
}
