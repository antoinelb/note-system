# Index schema: the path is the key, debt is representable

## Context

Phase 3 queries: notes by category/type/tag, backlinks, dangling links, typeless notes.
Phase 2 decided malformed metadata is data (`2026-07-anomalies-meta-donnees-fines.md`): a note may have no id, no type, or share an id with another note, and the index must store that, never reject it.

## Decision

Four tables, mirroring the domain types:

```sql
notes(path PRIMARY KEY, category NOT NULL, id, type, created, origin)
links(source_path REFERENCES notes ON DELETE CASCADE, target_id)  -- one row per #l occurrence
tags(note_path REFERENCES notes ON DELETE CASCADE, tag)
anomalies(note_path REFERENCES notes ON DELETE CASCADE, kind, field, raw)
```

- **`path` is the primary key** — the only identity the filesystem guarantees; `id` is nullable and deliberately **not** UNIQUE (duplicate ids are visible debt, queried rather than rejected).
- **`links.target_id` has no foreign key** — a FK would turn dangling links into insert errors; they are data for the open-loops panel, found with a `LEFT JOIN`.
- **One row per `#l` occurrence** — mirrors the parser's `Vec<Link>`, keeps rebuild a dumb insert loop, preserves occurrence counts; deduplication is `SELECT DISTINCT` at query time.
- **Anomalies as rows** (`kind` ∈ `duplicate-meta` / `invalid-created` / `malformed-field`, plus nullable `field` and `raw`) so the panel can join and count in SQL.
- **Typeless notes** = `type IS NULL AND category != 'capture'`: captures are typeless by design until promoted (their debt signal is "unsummarized", phase 8); notes with a missing `#meta` are included — their absent type is real debt and stays visible without a dedicated missing-meta query.
- No `MetaStatus` column: no phase-3 query needs it, and drop-and-rebuild (`2026-07-index-jetable-user-version.md`) makes adding columns later free.
- Positions and suggestions tables are v1/v3 — not created now; nothing in this schema blocks adding them.

## Alternatives rejected

- **`id` as primary key** — impossible: the id is `Option` in the domain and non-unique by design.
- **UNIQUE / FK constraints on ids and link targets** — turns visible debt into constraint violations during rebuild.
- **JSON columns for tags/anomalies** — moves filtering into Rust; as rows, tag and anomaly queries stay in SQL.
- **Deduplicating links on insert** — loses occurrence counts and adds conflict handling for no current need.
- **Typeless = raw `type IS NULL`** — every fresh capture would count as debt twice (typeless *and* unsummarized).
