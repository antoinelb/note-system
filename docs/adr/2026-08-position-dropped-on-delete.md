# A deleted note's position is dropped, not tombstoned

## Context

v1 phase 1 had to decide what happens to a position entry when its note is deleted.
Recreating an id later is possible — ids are kebab-from-title and frozen (`adr/2026-07-id-scheme-kebab-frozen.md`), so deleting `deep-modules` and later creating a note titled "Deep modules" yields the same id.

## Decision

**Drop on delete: deleting a note removes its position entry, and a recreated id starts unplaced.**

- The store exposes `Positions::remove(id)`; the in-app delete path calls it when permanent-note deletion lands (v1 phase 4).
- A recreated note is a new note, and it goes through the same placement path as any other new note (phase 8's auto-placement, once it exists) — inheriting the coordinates of a dead namesake was judged more surprising than starting fresh.
- Unknown ids in the file remain tolerated and preserved on load (they can arise from external deletion or hand edits); tolerance is a robustness property, not the deletion semantics.

## Alternatives rejected

- **Tombstone by omission (the store never deletes entries)** — zero coupling: phase 4's delete path would never need to know the store exists, and an accidental delete-then-recreate would keep its place.
  Rejected because dead entries accrete forever in a file meant to be read by hand, and the stale-place surprise outweighs the accident insurance.

## Consequence

Phase 4's delete path must call `remove` — a named coupling, accepted knowingly; this ADR is the reminder.
