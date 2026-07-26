# Index lifecycle: drop and rebuild, never migrate

## Context

The index schema will grow across versions (v1 positions, v3 suggestions).
The plan's first invariant: the index is derived and always rebuildable from the `.typ` files.

## Decision

The schema version is stamped with `PRAGMA user_version`.
On open: a version mismatch or a file that is not a readable SQLite database ⇒ delete the file and recreate it empty; the caller rebuilds from the vault.
The version is written only when creating (or recreating) the database — a clean open at the current version performs no writes, so a read-only index file can still be opened for querying.
No migration code, ever.

Consequence (already in the plan): canvas positions must **not** live in this file (v1 gives them separate storage), or they would die with every schema bump.

## Alternatives rejected

- **SQL migrations** — history and tooling for data this file does not own; pure overhead.
- **Repairing a corrupt file** — rebuild is cheaper and provably complete.
