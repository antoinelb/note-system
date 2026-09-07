# The table zooms continuously, and the semantic level is read off the scale

## Context

The table had exactly two scales — 1 for titles, 3 for bodies — reachable only by Ctrl+= and Ctrl+- (`adr/2026-08-body-zoom-scale-and-metrics.md`).
Two stops is a switch, not a zoom: there is no way to take in a wide constellation, no way to lean into one corner, and the wheel — the one gesture every map answers — did nothing at all.
The `Zoom` enum was the source of truth, so "which scale" and "how a card is drawn" were the same decision.

## Decision

- **The scale is an `f64` and the semantic level is derived from it.**
  `zoom` holds a scale; `table::Zoom::of(scale)` answers whether a card draws its title or its body.
  One source of truth, so culling and rendering cannot disagree — `in_view` takes the scale and picks the height the card actually draws at (`BODIES_AT` and above, `BODY_CARD_HEIGHT`).
- **Bodies start at scale 2.0** (`table::BODIES_AT`), between the two named stops, so a card is legible for a while before its body arrives.
- **The range is 0.25 ..= 4.0** and one notch is **×1.1** (`MIN_SCALE`, `MAX_SCALE`, `ZOOM_STEP`).
  Multiplicative, so a step out undoes a step in and every notch covers the same proportion of the range wherever the table stands; `table::stepped` clamps, and a notch at a bound writes nothing rather than refusing anything.
- **Ctrl+wheel zooms around the pointer; bare `+`/`=` and `-` zoom around the pane's centre.**
  A bare wheel still does what it did before, which is nothing.
  `table::rezoom` generalises to "keep this pane point still across a scale change": `pan' = pan + p·(1/s' − 1/s)`, the wheel passing the pointer and the keys the centre — the old viewport-centre rule is now the special case.
- **The two chords stay, as jumps** to exactly 1.0 and 3.0 (`Zoom::scale()`), with their palette rows unchanged: the semantic stops survive as named places on a continuous axis.
- **Zoom is refused while a sheet is open**, and opening one still lands the table at 1.0.
  The sheet, its tether and the raised card are viewport constructs whose math never sees a scale ≠ 1; making that a rule from both ends is cheaper than teaching three more constructs about the scale, and the note owns the keyboard there anyway.
- **A card answers its own wheel** and stops it there, the way it takes its own presses back: the pane reads `offsetX/offsetY` as pane-local, which they are only when the pane is the target, so a card computes the pane point from where it itself stands (`scale · (card + offset + pan)`).
- The scale is session state like the pan — nothing persists it.
- `spawn_position` divides the pane centre by the scale, like every other screen coordinate the canvas answers; the drop-resolution box (`resolve_drop`, `resolve_group`, 176 × 56 canvas units) is untouched and stays independent of the scale.

This supersedes `adr/2026-08-body-zoom-scale-and-metrics.md`'s "two semantic zoom levels" and its centre-only `rezoom`; the rest of that record — scale outside the translate, one division in `point()`, fixed clipped bodies, a sheet forcing titles — still stands.

## Rejected

- **Keeping `Zoom` as the stored state with the scale beside it** — two facts that must agree, which is the bug the derived level cannot have.
- **A linear step (±0.25)** — zooming in feels slow far out and violent close in; the multiplicative step is what every map does.
- **Zooming around the pane centre for the wheel too** — the pointer is the thing the hand is already aiming; centre-anchored wheel zoom means chasing the target with the pan afterwards.
- **A third handler measuring the pane's rectangle at mount** — an async `get_client_rect` for a number two existing event paths already carry exactly.
- **Zooming with a sheet open** — the tether would point at a card whose screen position it computes at scale 1, and the sheet frame would drift off its own index card.
- **A visible zoom percentage** — the cards' own treatment is the readout; a number would be chrome that says nothing the map does not already show.
