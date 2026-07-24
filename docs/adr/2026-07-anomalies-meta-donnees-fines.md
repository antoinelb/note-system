# Meta anomalies are field-level data, never errors

## Context

Phase 2 parses `#meta` from arbitrary user files.
The plan requires malformed input to surface in the open-loops panel, which means the parser must produce *data* describing what is wrong, not fail.
The remaining question was granularity: does one bad field poison the whole meta?

## Decision

Anomalies are recorded at the finest level that keeps the rest of the metadata usable:

- **Duplicate `#meta`**: the first call wins; the duplicate is recorded as an anomaly (visible debt), never a crash.
- **Unknown `type`** (e.g. a typo): kept as `NoteType::Unknown(String)` — data, not an error, and distinguishable from a missing type.
- **Unparseable `created`**: the field becomes `None` and the anomaly records the raw text; `id`, `type`, `tags` stay usable.
- **Missing `#meta` entirely**: an explicit status variant (`MetaStatus::Missing`), per the roadmap.

## Alternatives rejected

- **First `#meta` wins silently** — the anomaly becomes invisible; contradicts "debt is always visible".
- **One bad field ⇒ whole meta malformed** — simpler status model, but throws away good fields the index and panel could still use (a note with a typo'd date would lose its type).
- **Strict type enum, unknown ⇒ malformed** — same problem: one typo makes the note fully typeless.
