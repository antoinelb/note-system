# Body zoom: scale 3, fixed clipped bodies, Ctrl+= / Ctrl+-

## Context

Phase 6 needs two semantic zoom levels — titles ⇄ rendered typst bodies — behind a keystroke.
The deck never drew the bodies state; it is designed here in the phase-6-v0 vocabulary, like the loops list was.
`point()` (ui.rs) has promised since phase 2 that "body zoom must divide by its scale here, and nowhere else".

## Decision

- **Ctrl+= zooms in to bodies, Ctrl+- back to titles** — universal zoom muscle memory; table-only chords, each with its palette entry ("zoom to bodies" / "zoom to titles"), the level already stood at hidden.
- **The zoom is a canvas transform: `scale(3) translate(pan)`, scale outermost, `transform-origin: 0 0`.**
  With that order the pan stays in canvas units and one division in `point()` keeps every drag and pan correct — the promise kept literally.
  Zooming keeps the canvas point under the viewport centre fixed: `pan' = pan + centre·(1/s' − 1/s)` (`table::rezoom`).
- **Cards stay 176px logical** (528 on screen); the body is the note's whole rendered SVG in a fixed, clipped area below label and title — body 240px, card 296px tall, ×4 throughout.
  The SVG fits by `width: 100%` and is never restyled: the template's own typography is the point.
- **Opening a sheet forces titles zoom.**
  The sheet, dim, tether and raised card are titles-zoom viewport constructs (`SHEET_LEFT` math never sees a scale ≠ 1); a click at body zoom zooms out and opens — one legible gesture.
- Click slop is measured in canvas units after the division — slightly more forgiving at zoom, deliberately.

## Rejected

- **`translate(pan) scale(s)`** — would need the division in the card-drag arm only, breaking `point()`'s one-place promise and splitting the coordinate story.
- **Natural-height bodies** — tall notes would dominate the canvas and card footprints would vary; a fixed clip keeps the constellation legible.
- **A third zoom level (far dots)** — the deck sketched one; the plan settled on two, and the chord pair leaves room without code.

## Superseded by `2026-09-a-card-is-always-its-title.md` (2026-09-07)

The body level is gone: a card draws its title at every scale, the chords
are notches (`2026-09-the-table-zooms-continuously.md`) and "zoom to
titles" is the one jump.
The transform order, the canvas-unit pan and the sheet forcing scale 1
stand.

The transform order this ADR fixed is amended by
`2026-09-the-canvas-zooms-with-css-zoom.md`: the scale is now CSS `zoom`,
the transform only translates.
