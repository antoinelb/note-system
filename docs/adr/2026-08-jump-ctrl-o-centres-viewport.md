# Ctrl+O jumps to a note and centres the viewport on it

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
