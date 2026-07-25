# Canvas positions live in their own file

## Context

`CLAUDE.md` states the invariant: canvas positions are user data disguised as index data and must survive index rebuilds.
`2026-07-index-jetable-user-version.md` makes the index deliberately disposable — dropped and rebuilt wholesale on a schema change or on anything ambiguous.
`2026-07-schema-index-sqlite.md` deferred the positions table with "positions and suggestions tables are v1/v3 — not created now".
`plan.md` § Storage layout left the fork open: "either a separate small positions file or excluded from 'derived' purges".

The design pass (`docs/design/ui-spec-table-and-logs.md`) confirms positions are user-visible state, not a layout hint: "positions are persistent and only change when the user moves a card".

## Decision

Positions live in **their own file under `.index/`**, separate from the index database.

- The index database stays literally disposable: deleting or rebuilding it cannot lose a position.
- Written on move (debounced), read when the table loads.
- A missing entry means "not yet placed" — auto-placement decides, and the note is not lost.

The reasoning: user data has no upstream to rebuild from.
The very property that makes `DROP TABLE` safe for the index is what makes it unsafe for positions.
Separating the files makes the invariant structural instead of remembered.

Implementation is v1 (the table); the decision is recorded now, while the reasoning is fresh, so the phase-3 schema ADR's open promise is closed.

## Alternatives rejected

- **A positions table inside the index, excluded from the purge** — one file, but the invariant then depends on rebuild code remembering an exception forever; a future "just drop the db and rebuild" fix silently destroys user data, which is exactly the failure the invariant exists to prevent.
- **Positions in the vault (e.g. `vault/positions.toml`)** — survives even deleting `.index/`, and sits with the other user data, but it puts a non-note file in the vault, which the vanilla typst CLI and any Claude Code session then have to know to ignore. The vault stays "notes and templates".
- **Positions as `#meta` fields** — a canvas drag would rewrite note files, thrashing the watcher and the SVG cache on every mouse move, and making the file mtime meaningless as a content signal.
