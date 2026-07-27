# Id scheme: kebab-case of the title, frozen at creation

## Context

Notes need stable ids for `#l(id)` links, and files need names.
Decided during phase 1 (scheduled as a phase-5 ADR) because sample notes needed names anyway.

## Decision

- `id` = kebab-case of the note's title **at creation time**, then frozen: renaming a note's title never changes its id.
- Filename = `<id>.typ`; the id appears identically in `#meta(id: ...)`.
- Time notes use the date as both id and title (`2026-07-23`); weekly/season notes follow the same pattern with their period.
- ~~Collisions at creation get a numeric suffix (`-2`).~~ Superseded: a collision is a creation error (`adr/2026-07-id-collision-is-an-error.md`).

## Alternatives rejected

- **Timestamp ids (Luhmann-style)** — maximally stable but opaque; readability of `#l("...")` in source, autocomplete, and backlinks is worth more, and the index handles lookup anyway.
- **Id tracks the title on rename** — every rename dangles all inbound links until repaired; needs rename-repair tooling v0 doesn't have. Frozen ids cost only cosmetic title/id drift.

## Follow-up

Planned (unscheduled, post-v0): an explicit **auto-rename** action — re-derive the id from the current title, rename the file, rewrite inbound `#l` links via the index. Frozen-by-default stays; auto-rename is opt-in per note, never automatic on title edit.
