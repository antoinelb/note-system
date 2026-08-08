# The sheet's layers stack by DOM order, and the tether is a line of CSS

## Context

Wireframe 6b wants the table dimmed under the sheet with the origin card
still lit above the dim and a tether edge running card → sheet.
The `.canvas` is translated, and a CSS transform makes it a stacking
context: no child card can z-index its way above a sibling overlay drawn
outside the canvas.

## Decision

- **Paint order is DOM order — no z-index.** Inside `.table`, after
  `.canvas`: the `div.dim`, the raised origin card, the `div.tether`, then
  `aside.sheet`.
  Later siblings paint over the transformed canvas without fighting its
  stacking context; the palette keeps the app's only `z-index`.
- **The origin card leaves the canvas and re-renders raised.** While its
  sheet is open the canvas loop skips it and a copy renders as a `.table`
  child at viewport coordinates (`card.x + pan.x`), with the `--selection`
  border and the same grab seed — dragging the raised card is the same drag.
  The card markup stays inline in the canvas loop (its `key` must sit
  directly on the loop's node or the keyed diff degrades to in-place
  patching); only the grab-seeding closure is shared with the raised copy.
- **The sheet is viewport-anchored at the mockup's frame**: left 440 px,
  width `SHEET_W`, top/bottom inset 44 px — Rust consts in `table.rs`,
  because the tether math needs them.
  The deck's 6b mockup draws the tether as a single horizontal 1.2 px line
  at the card's mid-height, so the tether is one absolutely-positioned div,
  its geometry from the pure `table::tether(card, pan)`: nearest card edge
  to nearest sheet edge, width clamped to zero when the card sits under the
  sheet (drawn as nothing, no branch).
- **The dim has no handlers.** Its presses fall through to the pane, so the
  table still pans under the open sheet — which is also what makes "the
  tether tracks the card" observable.
  The sheet, in contrast, stops mousedown: a text selection inside it must
  never become a pan.
  If the watcher removes the open note, raised card and tether simply stop
  rendering; the sheet and its buffer hold.

## Rejected

- **z-index above the canvas** — dead on arrival: the transform's stacking
  context caps every canvas child regardless of z-index.
- **The dim inside the canvas, pan-compensated** — inverse-translating an
  overlay to stand still inside a moving layer, to avoid re-rendering one
  card.
- **A sheet positioned relative to its card** — the mockup anchors the
  sheet at x = 440 and lets the tether carry "beside its card"; a moving
  sheet would need viewport measurement and clamping for zero design gain.
- **SVG for the tether** — a one-line `<div>` with a background colour does
  it; curves and routing are explicitly out (plan § Canvas).
