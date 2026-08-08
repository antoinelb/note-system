# The table mounts: the walking skeleton's shape

## Context

v1 phase 2 mounts `Screen::Table` (wireframe state 6a): positioned cards at titles zoom on a pannable void.
Beyond the two decisions with their own ADRs (`2026-08-screen-switch-gesture.md`, `2026-08-light-table-colours-derived.md`), the skeleton fixed several smaller shapes that should not live only in code.

## Decisions

- **Id-less notes stay off the table.** Positions are keyed by id (`adr/2026-08-positions-plain-lines-file.md`), so a note without one cannot be placed; it remains visible debt in the loops list rather than an unplaceable card. The query (`Index::table_notes`) filters `id IS NOT NULL` alongside `category != 'time'`.
- **Unplaced notes stack on a 4-wide grid at the canvas origin**, ordered by id (the query's ORDER BY), ×4 spacing — deterministic and visible where the viewport starts, dumb and honest until phase 8's auto-placement. Rank counts only unplaced notes, so a drag out of the grid closes it up on the next load.
- **The positions flush joins the one `QuitFlush` callback.** Ctrl+Q flushes the open note and the positions store together; either failure holds the app open with a notice — one registration point, both stores.
- **Client deltas are canvas deltas.** The canvas is translated, never scaled, so pan and drag math subtracts client coordinates directly; phase 6's body zoom must divide by its scale in `point()`'s consumers and nowhere else.
- **The star field is CSS background, not DOM** — six one-pixel radial gradients on `.table`. Zero listener churn, and distant stars excusably don't parallax with the pan.
- **Save errors ride the editor notice** — the app's one message channel, rendered in the logs centre pane. A failed positions write is visible next time the logs are up; a dedicated table-side notice can come when daily driving demands it.
- **The pan offset is session state**, not persisted — only card positions are user data; the viewport starts at the origin, where the unplaced grid stands.

## Alternatives rejected

- **A path-keyed table including id-less notes** — would put cards on the table that cannot keep a position, and positions keyed two ways is exactly the ambiguity the plain-lines file avoids.
- **Persisting the pan/viewport** — nothing in the wireframes asks for it, and a stable origin plus phase 7's jump-to-note covers re-finding the map.
- **A second `QuitFlush` slot for positions** — two registration cells for one chord invites them to disagree on whether the app may close.
