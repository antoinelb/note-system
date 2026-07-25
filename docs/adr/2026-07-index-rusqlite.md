# Index crate: rusqlite (bundled)

## Context

Phase 3 stores the derived index in SQLite under `vault/.index/`.
The index is local, single-user, and every query touches at most a few thousand rows.

## Decision

`rusqlite` with the `bundled` feature: synchronous calls, SQLite compiled into the binary — no system dependency, pinned SQLite version.

## Alternatives rejected

- **sqlx** — async infects every caller for queries that take microseconds on a local file, and compile-time query checking needs a database at build time.
- **diesel** — an ORM plus a migration system; migrations are useless here (see `2026-07-index-jetable-user-version.md`) and the ORM hides SQL worth keeping obvious.
