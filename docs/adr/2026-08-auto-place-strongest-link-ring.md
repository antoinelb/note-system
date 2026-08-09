# Auto-placement: a derivation-time proposal on a ring, never a store write

## Context

Phase 8: an unplaced note should land near its strongest linked placed card, deterministically and bounded — no iteration to convergence at place time — and a hand-placed card must never be moved by the mechanism, test-enforced.
Phase 4's creation wrote a viewport-centre position into the store, which would have made every created note hand-placed at birth and the exit criterion ("creating a linked note lands it where it belongs") unreachable.

## Decision

- **Auto-placement is computed at card derivation, never written to the store** — like the origin fallback grid before it.
  The positions file thus holds only what the user dragged or explicitly arranged, and the hand-placed invariant is structural: nothing that could move a hand-placed card exists on the write path.
  An auto-placed card follows its links live as they change; dragging it once pins it forever.
- **Strength** = the count of link edges in either direction between the unplaced note and each anchor; the highest count wins, ties break to the lexicographically smallest anchor id — deterministic regardless of iteration order.
  **Anchors** are the store's positions plus cards auto-placed earlier in the same pass (id order), so chains cluster; origin-grid and birth-slot cards anchor nothing.
- **The slot** is the first free cell on a growing ring around the anchor: rings 1..=8, perimeter cells in row-major order, cell pitch 192×96 (the fallback grid's), "free" meaning no resolved card within one pitch.
  A saturated neighbourhood answers the last cell examined — total and bounded, never a search to convergence.
- **Creation's landing becomes a session birth slot**: `Fallback::place` pins the viewport centre in the session slot map instead of the store (supersedes the store write in `adr/2026-08-new-card-lands-at-viewport-centre.md`).
  A fresh note is linkless, so it stays at its birth slot; as links are written it drifts to its cluster; a relaunch before any drag resolves it by its links, or the origin grid.
- An unlinked note — or one whose links reach nothing anchored — keeps the origin fallback grid, as before.

## Rejected

- **Persisting auto-placement at creation or first appearance** — every card would be hand-placed within a session of existing, and the invariant would depend on remembering which writes are allowed.
- **Anchoring on fallback-grid cards** — clustering next to a parking lot; the grid is a queue, not a place.
- **Outgoing links only** — a note heavily linked *to* would ignore its natural cluster.
