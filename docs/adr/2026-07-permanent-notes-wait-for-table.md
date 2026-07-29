# Permanent notes have no in-app surface until the v1 table

## Context

The phase-4 note list was the only way to open, create or delete permanent
notes, and phase 7 deletes it — the finished app has no list, and the design
gives the "where knowledge lives" surface to the v1 table
(`adr/2026-07-two-screens-table-and-logs.md`).

## Decision

Between phase 7 and v1 the app touches **time notes only**.
Permanent, capture and generated notes stay plain files, editable outside
the app; the index keeps indexing them (backlinks, dangling links and the
open-loops count still see them), and phase 9's link autocomplete reads ids
from the index, not from any list.

## Rejected

- **A minimal off-design affordance** (keystroke-summoned open/create
  palette, a hidden create bar): code written now and deleted at v1, on a
  surface the design explicitly reserves for the table.
