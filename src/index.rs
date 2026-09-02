use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::domain::{
    Meta, MetaAnomaly, MetaStatus, Note, NoteCategory, NoteId, NoteType,
};
use crate::parse;
use jiff::{ToSpan, civil::Date};
use rusqlite::{Connection, Row, Transaction};

pub const SCHEMA_VERSION: i32 = 5;
const FOREIGN_KEYS: &str = "PRAGMA foreign_keys = on;";
const SCHEMA: &str = r#"
CREATE TABLE notes (
    path     TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    id       TEXT,
    type     TEXT,
    created  TEXT,
    due      TEXT,
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
CREATE VIRTUAL TABLE notes_fts USING fts5(
    path UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#;

/// The finder never lists more than this many hits: past a screenful,
/// one more word narrows better than scrolling does.
pub const MAX_HITS: i64 = 12;

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

/// One full-text hit: the note, its title when it has one, and the words
/// around the match (adr/2026-09-full-text-search-lives-in-the-index.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub title: Option<String>,
    pub snippet: String,
}

/// A note whose `due` day has come within the week or gone by — the loops
/// list names it overdue or due (adr/2026-09-course-type-and-due-loops.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueNote {
    pub path: PathBuf,
    pub due: Date,
}

/// A note linking *to* the one being read. The source's own id is carried
/// along because the footer labels backlinks by id, and an id-less source
/// (visible debt, still a real link) has to fall back to its filename.
#[derive(Debug, PartialEq, Eq)]
pub struct Backlink {
    pub source: PathBuf,
    pub id: Option<String>,
}

/// One note as the table draws it: everything a card needs except its
/// position, which lives in the plain-lines file the index can never touch
/// (adr/2026-08-positions-plain-lines-file.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableNote {
    pub id: String,
    /// Vault-relative, straight from the notes row — what body zoom reads
    /// and caches by (adr/2026-08-body-cache-per-note-svg.md).
    pub path: PathBuf,
    pub kind: NoteCategory,
    pub note_type: Option<NoteType>,
    pub title: Option<String>,
    /// ISO `YYYY-MM-DD` as the column stores it; parsed only where a
    /// capture's age is drawn.
    pub created: Option<String>,
    /// Sorted, straight from the tags table — what the tag filter matches
    /// (adr/2026-08-filter-overlay-ctrl-f.md).
    pub tags: Vec<String>,
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
            "DELETE FROM notes_fts;",
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

    /// Every link whose source has an id, as (source_id, target_id) pairs,
    /// deduplicated. Whether the target exists, stands on the table or
    /// dangles is the canvas geometry's lookup to miss, not SQL's — dangling
    /// stays queryable debt for the loops list
    /// (adr/2026-08-edges-svg-under-cards.md).
    pub fn link_edges(&self) -> Result<Vec<(String, String)>, IndexError> {
        query_rows(
            &self.connection,
            concat!(
                "SELECT DISTINCT sources.id, links.target_id ",
                "FROM links ",
                "JOIN notes AS sources ON sources.path = links.source_path ",
                "WHERE sources.id IS NOT NULL ",
                "ORDER BY sources.id, links.target_id"
            ),
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
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

    /// Every note the table can show, ordered by id: time is the one
    /// category that never appears on the canvas (adr/2026-07-two-screens-table-and-logs.md),
    /// and an id-less note cannot sit on it — positions are keyed by id,
    /// so it stays open-loops debt instead. Duplicate ids are returned
    /// as-is: a collision is an error to see, not to hide
    /// (adr/2026-07-id-collision-is-an-error.md). The category is derived
    /// from the path's leading directory for `captured_on`'s reason: the
    /// WHERE clause already constrains the column, and re-reading it would
    /// only add an untestable decode branch.
    pub fn table_notes(&self) -> Result<Vec<TableNote>, IndexError> {
        // tags gathered by a second statement and merged by path — no
        // GROUP_CONCAT delimiter gamble (adr/2026-08-filter-overlay-ctrl-f.md)
        let mut tags: HashMap<PathBuf, Vec<String>> = HashMap::new();
        for (path, tag) in query_rows(
            &self.connection,
            "SELECT note_path, tag FROM tags ORDER BY note_path, tag",
            [],
            |row| {
                Ok((
                    PathBuf::from(row.get::<_, String>(0)?),
                    row.get::<_, String>(1)?,
                ))
            },
        )? {
            tags.entry(path).or_default().push(tag);
        }
        query_rows(
            &self.connection,
            concat!(
                "SELECT id, path, type, title, created FROM notes ",
                "WHERE category != 'time' AND id IS NOT NULL ",
                "ORDER BY id"
            ),
            [],
            |row| {
                let path = PathBuf::from(row.get::<_, String>(1)?);
                let kind = if path.starts_with("capture") {
                    NoteCategory::Capture
                } else if path.starts_with("generated") {
                    NoteCategory::Generated
                } else {
                    NoteCategory::Permanent
                };
                let tags = tags.remove(&path).unwrap_or_default();
                Ok(TableNote {
                    id: row.get::<_, String>(0)?,
                    path,
                    kind,
                    note_type: row
                        .get::<_, Option<String>>(2)?
                        .map(|name| NoteType::from_name(&name)),
                    title: row.get::<_, Option<String>>(3)?,
                    created: row.get::<_, Option<String>>(4)?,
                    tags,
                })
            },
        )
    }

    /// Every distinct tag in the vault — the filter overlay's tag half
    /// (adr/2026-08-filter-overlay-ctrl-f.md).
    pub fn tag_names(&self) -> Result<Vec<String>, IndexError> {
        query_rows(
            &self.connection,
            "SELECT DISTINCT tag FROM tags ORDER BY tag",
            [],
            |row| row.get(0),
        )
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

    /// The notes whose text holds every word of `query`, best match
    /// first, at most `MAX_HITS` of them. The query is taken literally —
    /// each whitespace-separated word is one quoted FTS5 term, so a
    /// student's `"` or `*` never becomes syntax — and diacritics fold, so
    /// `idee` finds `idée`. An empty query finds nothing rather than
    /// everything (adr/2026-09-full-text-search-lives-in-the-index.md).
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, IndexError> {
        let Some(terms) = fts_terms(query) else {
            return Ok(Vec::new());
        };
        query_rows(
            &self.connection,
            concat!(
                "SELECT path, title, snippet(notes_fts, 2, '', '', '…', 10) ",
                "FROM notes_fts WHERE notes_fts MATCH ?1 ",
                "ORDER BY rank, path LIMIT ?2"
            ),
            rusqlite::params![terms, MAX_HITS],
            |row| {
                Ok(SearchHit {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    title: row.get::<_, Option<String>>(1)?,
                    // snippet() builds text; there is no stored value it
                    // could hand back unread
                    snippet: row.get::<_, String>(2).unwrap_or_default(),
                })
            },
        )
    }

    /// Every note due on or before `today` plus seven days, soonest first:
    /// the overdue ones and the ones due this week, which the loops list
    /// tells apart against `today`. `due` is stored as `YYYY-MM-DD` text,
    /// so the string comparison is the date comparison. A note with no
    /// `due` owes nothing here.
    pub fn due_notes(&self, today: Date) -> Result<Vec<DueNote>, IndexError> {
        // the calendar's far edge is the only way a week from today fails;
        // there the horizon is today itself and nothing future is listed
        let horizon = today.checked_add(7.days()).unwrap_or(today);
        query_rows(
            &self.connection,
            concat!(
                "SELECT path, due FROM notes ",
                "WHERE due IS NOT NULL AND due <= ?1 ",
                "ORDER BY due, path"
            ),
            [horizon],
            |row| {
                Ok(DueNote {
                    path: PathBuf::from(row.get::<_, String>(0)?),
                    due: row.get::<_, Date>(1)?,
                })
            },
        )
    }

    /// Every note carrying an anomaly, one row per (note, family) — the
    /// malformed `#meta` the parser recorded, the truncations the walk
    /// hit — read back for the loops list
    /// (adr/2026-08-anomalies-join-the-loops.md); until then the table
    /// was write-only. The family is already the loops-list label.
    pub fn anomalies(&self) -> Result<Vec<(PathBuf, String)>, IndexError> {
        let kinds = query_rows(
            &self.connection,
            concat!(
                "SELECT DISTINCT note_path, kind ",
                "FROM anomalies ORDER BY note_path, kind"
            ),
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        // the kinds fold into the two loops-list families, deduplicated in
        // query order — a note with five malformed fields is one problem
        // ponytail: linear contains — the list is as small as the debt
        let mut families: Vec<(PathBuf, String)> = Vec::new();
        for (path, kind) in kinds {
            let family = if kind == "truncated" {
                "truncated"
            } else {
                "malformed meta"
            };
            let entry = (PathBuf::from(path), family.to_string());
            if !families.contains(&entry) {
                families.push(entry);
            }
        }
        Ok(families)
    }
}

/// A literal query as FTS5 wants it: every word its own quoted term, a
/// quote inside a word doubled, and the terms joined by FTS5's implicit
/// AND. `None` for a query with no word in it.
fn fts_terms(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|word| format!("\"{}\"", word.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// What of a note's text is worth finding: everything past the preamble.
/// Every template opens with `#import`, `#show` and `#meta`, and ends that
/// run with a blank line before the title, so a note whose first line is
/// an import loses everything up to its first blank line; a note that
/// opens any other way is indexed whole. Words like `template` or `meta`
/// therefore hit only the notes that actually say them.
fn searchable(source: &str) -> &str {
    if !source.starts_with("#import") {
        return source;
    }
    // the two bytes found are the two bytes stepped over, so the slice
    // cannot start past the end
    source.find("\n\n").map_or("", |blank| &source[blank + 2..])
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
                let source = std::fs::read_to_string(path.path())?;
                let parsed_note = parse::parse_note(&source);

                notes.push(Note {
                    path: Path::new(category.as_dir()).join(&name),
                    category,
                    meta: parsed_note.meta,
                    title: parsed_note.title,
                    links: parsed_note.links,
                    summarized: parsed_note.summarized,
                    truncated: parsed_note.truncated,
                    source,
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
    // a virtual table knows no foreign key: its row goes by hand
    transaction.execute(
        "DELETE FROM notes_fts WHERE path = ?1",
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
            "(path, category, id, type, created, due, origin, title, ",
            "summarized) ",
            "VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        ),
        rusqlite::params![
            note.path.to_string_lossy(),
            note.category.as_dir(),
            meta.id.as_ref().map(|id| id.0.as_str()),
            meta.note_type.as_ref().map(NoteType::as_name),
            meta.created,
            meta.due,
            meta.origin.as_deref(),
            note.title.as_deref(),
            note.summarized,
        ],
    )?;
    transaction.execute(
        concat!(
            "INSERT INTO notes_fts (path, title, body)",
            "VALUES (?1, ?2, ?3)"
        ),
        rusqlite::params![
            note.path.to_string_lossy(),
            note.title.as_deref(),
            searchable(&note.source),
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
            MetaAnomaly::InvalidDue(raw) => {
                ("invalid-due", None, Some(raw.as_str()))
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
    // note-level, not a meta anomaly: a truncated walk may have missed the
    // `#meta` itself (adr/2026-08-anomalies-join-the-loops.md)
    if note.truncated {
        transaction.execute(
            concat!(
                "INSERT INTO anomalies (note_path, kind, field, raw)",
                "VALUES (?1, 'truncated', NULL, NULL)"
            ),
            rusqlite::params![note.path.to_string_lossy()],
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
    fn table_notes_carry_their_tags_and_tag_names_deduplicate() {
        let (_dir, mut index) = temp_index();
        let notes = scan_vault(&fixture_vault()).expect("scan fixture");
        index.rebuild(&notes).expect("rebuild");

        let rows = index.table_notes().expect("query");
        let tags_of = |id: &str| {
            rows.iter()
                .find(|note| note.id == id)
                .map(|note| note.tags.clone())
                .expect("the note is on the table")
        };
        assert_eq!(tags_of("zettelkasten"), vec!["method".to_string()]);
        assert_eq!(tags_of("luhmann"), Vec::<String>::new());

        // "method" appears on two notes and once in the vocabulary
        assert_eq!(
            index.tag_names().expect("query"),
            vec![
                "book".to_string(),
                "math".to_string(),
                "method".to_string(),
                "rendering".to_string(),
                "rust".to_string()
            ]
        );
    }

    #[test]
    fn tag_queries_report_rows_that_will_not_decode() {
        // one blob per column read: the merge statement's two, and
        // tag_names' one
        for plant in [
            "INSERT INTO notes (path, category) \
             VALUES (x'00', 'permanent'); \
             INSERT INTO tags (note_path, tag) VALUES (x'00', 'ok');",
            "INSERT INTO notes (path, category) \
             VALUES ('permanent/a.typ', 'permanent'); \
             INSERT INTO tags (note_path, tag) \
             VALUES ('permanent/a.typ', x'00');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the blob row");
            assert!(matches!(index.table_notes(), Err(IndexError::Sqlite(_))));
        }
        // tag_names decodes only the tag column
        let (dir, index) = temp_index();
        let raw = Connection::open(dir.path().join("index.sqlite"))
            .expect("raw open");
        raw.execute_batch(
            "INSERT INTO notes (path, category) \
             VALUES ('permanent/a.typ', 'permanent'); \
             INSERT INTO tags (note_path, tag) \
             VALUES ('permanent/a.typ', x'00');",
        )
        .expect("plant the blob row");
        assert!(matches!(index.tag_names(), Err(IndexError::Sqlite(_))));
    }

    #[test]
    fn link_edges_lists_deduplicated_id_pairs_without_idless_sources() {
        let (dir, mut index) = temp_index();
        let notes = scan_vault(&fixture_vault()).expect("scan fixture");
        index.rebuild(&notes).expect("rebuild");
        // debt rows stay invisible or collapse: a link from the id-less
        // note, and a duplicate of a pair the vault already has
        let raw = Connection::open(dir.path().join("index.sqlite"))
            .expect("raw open");
        raw.execute_batch(concat!(
            "INSERT INTO links (source_path, target_id) ",
            "VALUES ('permanent/missing-meta.typ', 'zettelkasten');",
            "INSERT INTO links (source_path, target_id) ",
            "VALUES ('permanent/luhmann.typ', 'zettelkasten');",
        ))
        .expect("plant the debt rows");

        let pair = |source: &str, target: &str| {
            (source.to_string(), target.to_string())
        };
        assert_eq!(
            index.link_edges().expect("query"),
            vec![
                pair("2026-07-21", "2026-07-22"),
                pair("2026-07-21", "zettelkasten"),
                pair("2026-07-22", "2026-07-21"),
                pair("2026-07-22", "2026-07-23"),
                pair("2026-07-23", "2026-07-22"),
                pair("2026-07-23", "smart-notes"),
                pair("2026-w30", "2026-07-21"),
                // the dangling target rides along — the canvas geometry,
                // not SQL, is what draws nothing for it
                pair("atomic-notes", "evergreen-notes"),
                pair("atomic-notes", "zettelkasten"),
                pair("capture-idea-canvas", "note-system"),
                pair("devoir-1", "analyse-reelle"),
                pair("digest-smart-notes", "smart-notes"),
                pair("link-traps", "zettelkasten"),
                pair("luhmann", "zettelkasten"),
                pair("note-system", "plain-files"),
                pair("note-system", "zettelkasten"),
                pair("plain-files", "note-system"),
                pair("smart-notes", "luhmann"),
                pair("zettelkasten", "atomic-notes"),
                pair("zettelkasten", "luhmann"),
            ]
        );
    }

    #[test]
    fn link_edges_report_rows_that_will_not_decode() {
        // one blob per column read, so each `?` in the closure fires; a
        // blob id passes IS NOT NULL, which is exactly the decode to catch
        for plant in [
            "INSERT INTO notes (path, category, id)
             VALUES ('permanent/blob-id.typ', 'permanent', x'00');
             INSERT INTO links (source_path, target_id)
             VALUES ('permanent/blob-id.typ', 'target');",
            "INSERT INTO notes (path, category, id)
             VALUES ('permanent/source.typ', 'permanent', 'source');
             INSERT INTO links (source_path, target_id)
             VALUES ('permanent/source.typ', x'00');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the blob row");
            assert!(matches!(index.link_edges(), Err(IndexError::Sqlite(_))));
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
    fn table_notes_lists_everything_except_time_and_the_id_less() {
        let (_dir, mut index) = temp_index();
        let notes = scan_vault(&fixture_vault()).expect("scan fixture");
        index.rebuild(&notes).expect("rebuild");

        let rows = index.table_notes().expect("query");
        // the fixture's 16 non-time notes minus missing-meta.typ, whose
        // absent id keeps it off the table and in the loops list
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "analyse-reelle",
                "atomic-notes",
                "capture-articles-zettel",
                "capture-idea-canvas",
                "devoir-1",
                "digest-smart-notes",
                "duplicate-meta",
                "link-traps",
                "luhmann",
                "missing-type",
                "note-system",
                "plain-files",
                "quotes",
                "smart-notes",
                "zettelkasten",
            ]
        );
        let kind_of = |id: &str| {
            rows.iter().find(|row| row.id == id).map(|row| row.kind)
        };
        assert_eq!(kind_of("zettelkasten"), Some(NoteCategory::Permanent));
        assert_eq!(
            kind_of("capture-idea-canvas"),
            Some(NoteCategory::Capture)
        );
        assert_eq!(
            kind_of("digest-smart-notes"),
            Some(NoteCategory::Generated)
        );
        let missing_type = rows
            .iter()
            .find(|row| row.id == "missing-type")
            .expect("missing-type row");
        assert_eq!(missing_type.note_type, None);
        assert_eq!(missing_type.title.as_deref(), Some("Note without a type"));
        assert_eq!(missing_type.created.as_deref(), Some("2026-07-23"));
        let zettelkasten = rows
            .iter()
            .find(|row| row.id == "zettelkasten")
            .expect("zettelkasten row");
        assert_eq!(zettelkasten.note_type, Some(NoteType::Concept));
    }

    #[test]
    fn table_notes_reports_rows_that_will_not_decode() {
        // one blob per column read, so each `?` in the closure fires
        for plant in [
            "INSERT INTO notes (path, category, id)
             VALUES (x'00', 'permanent', 'ok');",
            "INSERT INTO notes (path, category, id)
             VALUES ('permanent/blob-id.typ', 'permanent', x'00');",
            "INSERT INTO notes (path, category, id, type)
             VALUES ('permanent/blob-type.typ', 'permanent', 'ok', x'00');",
            "INSERT INTO notes (path, category, id, title)
             VALUES ('permanent/blob-title.typ', 'permanent', 'ok', x'00');",
            "INSERT INTO notes (path, category, id, created)
             VALUES ('permanent/blob-created.typ', 'permanent', 'ok', x'00');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the blob row");
            assert!(matches!(index.table_notes(), Err(IndexError::Sqlite(_))));
        }
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
    fn a_refused_truncation_row_surfaces_on_its_own() {
        // rich_note's meta anomalies would hit the refusal first; a note
        // whose only anomaly is the truncation reaches the second insert
        let (_dir, mut index) = temp_index();
        refuse(
            &index,
            move |action| matches!(action, AuthAction::Insert { table_name } if *table_name == "anomalies"),
        );
        let mut bare = rich_note();
        if let MetaStatus::Present(meta) = &mut bare.meta {
            meta.anomalies.clear();
        }
        assert!(matches!(index.rebuild(&[bare]), Err(IndexError::Sqlite(_))));
    }

    #[test]
    fn anomalies_report_rows_that_will_not_decode() {
        // one blob per column read, so each `?` in the closure fires; the
        // pragma lets the orphan rows land without a parent note
        for plant in [
            "PRAGMA foreign_keys = off;
             INSERT INTO anomalies (note_path, kind)
             VALUES (x'00', 'malformed-field');",
            "PRAGMA foreign_keys = off;
             INSERT INTO anomalies (note_path, kind)
             VALUES ('permanent/host.typ', x'00');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the blob row");
            assert!(matches!(index.anomalies(), Err(IndexError::Sqlite(_))));
        }
    }

    #[test]
    fn due_notes_lists_the_week_ahead_and_everything_overdue_soonest_first() {
        let (_dir, mut index) = temp_index();
        let dated = |id: &str, due: Option<&str>| Note {
            path: PathBuf::from(format!("permanent/{id}.typ")),
            category: NoteCategory::Permanent,
            meta: MetaStatus::Present(Meta {
                id: Some(NoteId(id.to_string())),
                note_type: Some(NoteType::Project),
                due: due.map(|day| day.parse().expect("a test date")),
                ..Meta::default()
            }),
            title: None,
            links: vec![],
            summarized: true,
            truncated: false,
            source: String::new(),
        };
        index
            .rebuild(&[
                dated("late", Some("2026-07-20")),
                dated("today", Some("2026-07-24")),
                dated("week", Some("2026-07-31")),
                dated("later", Some("2026-08-01")),
                dated("free", None),
            ])
            .expect("rebuild");
        let today = jiff::civil::date(2026, 7, 24);
        let listed: Vec<(String, String)> = index
            .due_notes(today)
            .expect("the due read")
            .into_iter()
            .map(|note| {
                (crate::domain::stem_of(&note.path), note.due.to_string())
            })
            .collect();
        assert_eq!(
            listed,
            vec![
                ("late".to_string(), "2026-07-20".to_string()),
                ("today".to_string(), "2026-07-24".to_string()),
                ("week".to_string(), "2026-07-31".to_string()),
            ],
            "a week out is listed, a day past it and the undated are not"
        );
    }

    #[test]
    fn a_due_row_that_will_not_decode_fails_the_due_read() {
        // either column: a blob where the path should be, or text that
        // sorts before the horizon but is no date (a blob due would sort
        // after every text and never be selected at all). The row is
        // reported, not skipped
        for plant in [
            "INSERT INTO notes (path, category, due) \
             VALUES (x'00', 'permanent', '2026-07-01');",
            "INSERT INTO notes (path, category, due) \
             VALUES ('permanent/bad-due.typ', 'permanent', '0000-99-99');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the undecodable row");
            let error = index
                .due_notes(jiff::civil::date(2026, 7, 24))
                .expect_err("the row does not decode");
            assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
        }
    }

    #[test]
    fn search_finds_words_past_the_preamble_folding_diacritics() {
        let (_dir, mut index) = temp_index();
        index.rebuild(&[rich_note()]).expect("seed the index");
        let hits = index.search("quokka").expect("the search reads");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, PathBuf::from("permanent/rich.typ"));
        assert_eq!(hits[0].title.as_deref(), Some("A rich note"));
        assert!(hits[0].snippet.contains("quokka"), "{:?}", hits[0].snippet);

        // `idee` finds `idée`; two words are both required
        assert_eq!(index.search("idee").expect("reads").len(), 1);
        assert_eq!(index.search("idee quokka").expect("reads").len(), 1);
        assert_eq!(index.search("idee wombat").expect("reads").len(), 0);
        // the preamble is not indexed: `template` and `meta` hit nothing
        assert_eq!(index.search("template").expect("reads").len(), 0);
        assert_eq!(index.search("meta").expect("reads").len(), 0);
        // a query with no word in it finds nothing, not everything
        assert_eq!(index.search("   ").expect("reads").len(), 0);
        // FTS5 syntax in the query is text, never an operator: the
        // tokenizer drops the punctuation and the words stay required
        assert!(index.search("quokka* AND (x OR y) NOT").is_ok());
        assert_eq!(index.search("quokka* (wombat)").expect("reads").len(), 0);
        assert_eq!(index.search("\"quokka").expect("reads").len(), 1);
    }

    #[test]
    fn search_follows_updates_and_deletes() {
        let (_dir, mut index) = temp_index();
        index.rebuild(&[rich_note()]).expect("seed the index");
        let mut rewritten = rich_note();
        rewritten.source = "no preamble here: wallaby\n".to_string();
        index.update_note(&rewritten).expect("update");
        assert_eq!(index.search("quokka").expect("reads").len(), 0);
        assert_eq!(index.search("wallaby").expect("reads").len(), 1);
        index
            .remove_note(Path::new("permanent/rich.typ"))
            .expect("remove");
        assert_eq!(index.search("wallaby").expect("reads").len(), 0);
    }

    #[test]
    fn a_search_row_that_will_not_decode_fails_the_search() {
        // a blob where the path or the title should be: reported, not
        // skipped, like every other query's undecodable row
        for plant in [
            "INSERT INTO notes_fts (path, title, body) \
             VALUES (x'00', 'Titled', 'quokka');",
            "INSERT INTO notes_fts (path, title, body) \
             VALUES ('permanent/x.typ', x'00', 'quokka');",
        ] {
            let (dir, index) = temp_index();
            let raw = Connection::open(dir.path().join("index.sqlite"))
                .expect("raw open");
            raw.execute_batch(plant).expect("plant the undecodable row");
            let error =
                index.search("quokka").expect_err("the row does not decode");
            assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
        }
    }

    #[test]
    fn a_vanished_search_table_fails_the_search_and_both_writes() {
        let (dir, mut index) = temp_index();
        let raw = Connection::open(dir.path().join("index.sqlite"))
            .expect("raw open");
        raw.execute_batch("DROP TABLE notes_fts")
            .expect("the sabotage succeeds");
        assert!(matches!(index.search("x"), Err(IndexError::Sqlite(_))));
        // the insert's FTS row is the write a rebuild trips on, the delete's
        // is the one an update trips on first
        assert!(matches!(
            index.rebuild(&[rich_note()]),
            Err(IndexError::Sqlite(_))
        ));
        assert!(matches!(
            index.update_note(&rich_note()),
            Err(IndexError::Sqlite(_))
        ));
        // a stand-in that takes the delete and refuses every insert: the
        // one sabotage that reaches the insert's own error arm
        raw.execute_batch(
            "CREATE TABLE notes_fts (path TEXT CHECK (path IS NULL), \
             title, body)",
        )
        .expect("the stand-in is created");
        assert!(matches!(
            index.rebuild(&[rich_note()]),
            Err(IndexError::Sqlite(_))
        ));
    }

    #[test]
    fn query_terms_are_quoted_one_word_each() {
        assert_eq!(fts_terms("un  deux"), Some("\"un\" \"deux\"".to_string()));
        assert_eq!(
            fts_terms("say \"hi\""),
            Some("\"say\" \"\"\"hi\"\"\"".to_string())
        );
        assert_eq!(fts_terms(""), None);
        assert_eq!(fts_terms(" \t"), None);
    }

    #[test]
    fn the_searchable_text_starts_past_a_templates_preamble() {
        assert_eq!(
            searchable("#import x\n#show: note\n\n= T\nbody"),
            "= T\nbody"
        );
        assert_eq!(searchable("plain\n\ntext"), "plain\n\ntext");
        // an import with no blank line after it is all preamble
        assert_eq!(searchable("#import x\n#show: note\n"), "");
    }

    #[test]
    fn anomalies_read_back_one_row_per_note_and_family() {
        let (_dir, mut index) = temp_index();
        index.rebuild(&[rich_note()]).expect("seed the index");
        assert_eq!(
            index.anomalies().expect("the anomalies read"),
            vec![
                (
                    PathBuf::from("permanent/rich.typ"),
                    "malformed meta".to_string()
                ),
                (PathBuf::from("permanent/rich.typ"), "truncated".to_string()),
            ],
            "three meta anomalies fold into one family row"
        );
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
                due: None,
                anomalies: vec![
                    MetaAnomaly::DuplicateMeta,
                    MetaAnomaly::InvalidCreated("hier".to_string()),
                    MetaAnomaly::InvalidDue("bientôt".to_string()),
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
            truncated: true,
            source: concat!(
                "#import \"/templates/template.typ\": *\n",
                "#show: note\n",
                "#meta(id: \"rich\")\n",
                "\n= A rich note\n",
                "\nUne idée riche, and a word only this note says: quokka.\n",
            )
            .to_string(),
        }
    }
}
