# Positions file: plain `id x y` lines at `.index/positions`

## Context

`adr/2026-07-positions-separate-file.md` decided positions live in their own file under `.index/`, beside the database; v1 phase 1 implements it and had to pick a format and filename.
Positions are user data with no upstream to rebuild from, so the roadmap asked for something debuggable at 3 AM.
Cargo.toml carries no serialization dependency — no serde, no toml, no json — and hand-rolled parsing is already house style (`parse.rs`).

## Decision

**One entry per line, `id x y`, whitespace-separated, at `vault/.index/positions` (no extension).**

- Ids are kebab-case and frozen (`adr/2026-07-id-scheme-kebab-frozen.md`), so they never contain whitespace and the format needs no quoting.
- Coordinates are `f64`; Rust's `Display` prints the shortest string that round-trips exactly, so no precision format is needed and whole numbers stay clean (`340`, not `340.0`).
- Loading degrades, never crashes: a missing or unreadable file is "nothing placed", and a malformed line loses that line, not the file — a hand-edit typo cannot cost every position on the next save.
- Non-finite coordinates (`nan`, `inf`) parse as `f64` but are rejected as entries: they are not a place on the table.
- Saving is a plain `fs::write` of sorted lines — the same known ceiling as the editor's save (`adr/2026-07-debounced-autosave.md`): the crash window is one small write, and the upgrade path is write-temp-then-rename.
- The store (`src/positions.rs`) is synchronous like `Editor::save`; the debounced write timer belongs to the UI that mounts the table (v1 phase 2), mirroring how `ui.rs` owns the autosave timer.

## Alternatives rejected

- **TOML or JSON** — each drags the project's first serialization dependency in for a three-field line; the hand-rolled parser is under fifteen lines.
- **An extension'd filename (`positions.txt`, `positions.tsv`)** — no tooling keys off the extension, and the sibling is already extensionless-adjacent (`index.db` names its format because SQLite tooling does key off it).
- **Whole-file rejection on any malformed line** — simpler to state, but a save after a degraded load rewrites the file, so one typo would silently erase every position.
