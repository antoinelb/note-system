# A click opens the sheet, a drag moves the card

## Context

Phase 2's table knows only drags: a card mousedown seeds `Grab::Card`, the
pane's mousemove writes positions live, mouseup releases.
Phase 3 needs "click a card" to open its sheet without stealing the drag —
one gesture, two meanings, disambiguated somewhere.

## Decision

- **The whole travel decides, at mouseup.** `Grab::Card` gains a `down`
  field — the client point of the press, never mutated.
  On release, `table::is_click(down, up)` compares the total travel against
  `CLICK_SLOP` (4 px, max-norm): within it the press was a click and
  `open_sheet` runs; beyond it the press was the drag it always was.
- **A sub-slop wobble still writes its pixel or two.** The live
  `positions.write()` on every mousemove is untouched — a jittered click
  both opens the sheet and honestly keeps the 2 px move.
  The phase-2 guarantee stands unchanged: a click that never moves writes
  no position, because no mousemove ever fired.
- **Clicking the raised origin card re-opens nothing**: `open_sheet` returns
  early when the sheet already shows that id, so the press stays available
  as the start of a drag.

## Rejected

- **Deciding at mousedown with a timer or a move threshold armed later** —
  state and a race, where mouseup already knows the answer.
- **Zero tolerance (exactly no movement = click)** — real hands wobble; a
  1 px slip would silently demote every click to a drag.
- **Suppressing the position write inside the slop** — the write is the live
  repaint; buffering it until the click/drag verdict would add a branch to
  the one total mousemove handler to hide a movement the user actually
  made.
