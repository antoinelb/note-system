# Ctrl+O jumps to a note and centres the viewport on it

> **Superseded 2026-09-04** by
> `adr/2026-09-ctrl-o-is-the-one-note-switcher.md`: Ctrl+O now *opens* a
> note from any screen rather than panning the table's viewport onto its
> card, and the restriction to ids that have cards is gone with it —
> time notes and untyped notes are switchable. `table::centre_on` and the
> palette's "jump to note" row are removed. What survives is the matching
> rule this ADR chose: `links::filter` over ids and titles, Ctrl+L's own
> rule and cap.

## Context

Phase 7's jump-to-note: search ids and titles, pan the viewport to the card.
The keystroke and overlay shape were undrawn.

## Decision

- **Ctrl+O**, table-only — Obsidian's quick-switcher chord, existing muscle memory from the app this replaces.
- The overlay is the link picker's grammar over the completions query, **restricted to ids that have cards**: time notes and id-less notes can't be jumped to on a table that never hosts them.
  `links::filter` does the narrowing — ids and titles, the same rule and cap as Ctrl+L.
- **Enter pans so the card's nominal centre sits at the viewport centre, at the current zoom** (`table::centre_on`: `pan = centre/s − card_centre`) — no zoom change; jumping is about *where*, not *how close*.
- Fallback-slot cards jump too: the resolution runs through `table::cards`, so an unplaced note jumps to wherever it currently stands.

## Rejected

- **Ctrl+J** — self-describing but unlearned; the muscle memory exists.
- **Zooming to the card as part of the jump** — two decisions on one keystroke; the zoom pair is one chord away.
- **Highlighting the landed card** — a new visual state for something the centred viewport already says.
