# Viewport culling, sized by onresize

## Context

Phase 6 must answer the scale question ("low thousands"): only visible cards render, at either zoom level — body zoom otherwise compiles every note in the vault.
Culling needs the viewport's size, which the component cannot ask the window for headlessly.

## Decision

- **Culling is a pure predicate** (`table::in_view`): a card's rectangle, under `scale(zoom) translate(pan)`, intersects the viewport — per-zoom height bounds (`TITLE_CARD_HEIGHT` 96 conservative, `BODY_CARD_HEIGHT` 296), exact edges excluded.
  The canvas card loop filters on it; the raised card and the edges svg stay unculled (one card; cheap lines that may cross the viewport even when their cards don't).
- **The size comes from `onresize` on the `.table` pane** into a `viewport` signal — Dioxus's ResizeObserver-backed event, which fires immediately on observation and on every resize; until the first event the signal holds `table::DEFAULT_VIEWPORT` (1280×800).
- The phase-4 `Viewport` root-context closure stays what creation spawns from: it answers anywhere (the logs too, where no `.table` is mounted to observe), while the signal is the table's own reactive concern.

## Rejected

- **Reading the injected window-size closure per render for culling** — correct after the next interaction but blind to a resize itself; the observer is the reactive truth.
- **Culling edges** — an edge between two off-screen cards can cross the screen; the lines are cheap, the note compiles they gate are not.
