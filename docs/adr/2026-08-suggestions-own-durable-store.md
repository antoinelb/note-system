# Suggestions and dismissals live in their own durable store

## Context

The plan was that "AI suggestions live in `.index/`" and "suggestions are stored in the sidecar index only" — written before the index's disposability was made total: `Index::open` discards the database on any version mismatch, `rebuild()` deletes every table, and the app rebuilds on every start (`ui.rs::load_notes`).
`adr/2026-07-sqlite-index-schema.md` deferred the question with "positions and suggestions tables are v1/v3 — not created now"; positions got their answer in `adr/2026-07-positions-separate-file.md`.
Suggestions are AI output that costs a `claude` call to regenerate; dismissals are pure user judgment — "this connection is wrong" — with no upstream to rebuild from at any price.
A dismissed suggestion that resurfaces after a rebuild is the friction system proposing the same rejected link forever.

## Decision

Suggestions **and** dismissals live in their own store under `.index/`, separate from the index database, exactly as positions do.

- The index database stays literally disposable: dropping or rebuilding it cannot lose a suggestion or resurrect a dismissed one.
- Dismissals are user data disguised as index data — the positions invariant, extended; separating the files makes it structural instead of remembered.
- The store holds only what is *proposed* and what is *dismissed*; what is *linked* stays in the files, so a suggestion whose link now exists self-clears on re-index.
- The concrete shape (plain-lines file like `positions.rs` vs a second SQLite file) is decided at the phase-1 task, with the record shape.

## Alternatives rejected

- **Fully disposable, in the index** — the purest "index is derived" reading, but suggestions are derived from paid `claude` calls, not from the files, and dismissals are derived from nothing; every rebuild would empty the ambient layer and un-dismiss everything.
- **Dismissals durable, suggestion rows disposable** — protects the user data at a cheaper schema, but each app start empties the canvas of dashed edges until the next manual run, making the "free and ambient" discovery layer flicker with the index's lifecycle.
- **Suggestion tables inside the index, excluded from the purge** — the same trap the positions ADR named: the invariant then depends on rebuild code remembering an exception forever.
