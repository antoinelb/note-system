# Link edges are one SVG under the cards

## Context

Phase 5 draws real links as solid edges with node dots where an edge meets a card: straight lines, no routing, under the cards, following drags live, dangling drawing nothing.
The canvas already has a stacking rule (DOM order, `adr/2026-08-sheet-stacking-dom-order.md`) and a pan (a transform on `.canvas`).

## Decision

- **One `<svg class="edges">` as the canvas's first child.**
  DOM order paints it under every card, and living inside the translated canvas it pans and follows drags for free — the geometry re-derives from the same `positions` read that repaints the cards.
  The svg is a 1×1 box with `overflow: visible` (canvas coordinates are unbounded in every direction) and `pointer-events: none` (a press on an edge is the void's pan).
- **Geometry is pure** (`table::edges`): centre-to-centre lines, each endpoint slab-clipped to its card's nominal rectangle — half-extents `CARD_WIDTH/2` × `TETHER_DROP`, the one card height the layout already declares.
  The clipped endpoints are where the node dots sit.
  Clips that cross (overlapping or touching cards) and self-links draw nothing — the tether's zero-width idiom; the f64 arithmetic handles zero-length axes by itself (division by zero is infinity, which loses every `min`).
- **The index answers id pairs** (`Index::link_edges`): every link whose source has an id, as `(source_id, target_id)`, deduplicated.
  Whether the target exists or stands on the table is the geometry's lookup to miss, not SQL's — dangling stays queryable debt for the loops list, off the canvas.
- Edges ride the survey beside `table_notes`, so the watcher redraw comes free.
- Colours: one new variable, `--edge` (`#332c52` from the palette table's "Real link edges" row; the light sibling derived by the standing rule, silverpoint on the pale ground).
  The node dots reuse `--selection` — the wireframes name one role, "edge node dots / selection border", and one role stays one variable.

## Rejected

- **Per-edge absolutely-positioned divs** — a rotated div per line fights the layout for what SVG states directly.
- **Filtering placed/dangling in SQL** — the placed set lives in the store and the session fallback, which SQL cannot see; one lookup table in the geometry is the honest join.
- **Edge routing or curves** — plan.md § Canvas: straight lines, no routing; legibility comes from placement, not splines.
