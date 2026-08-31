# Text objects reach visual mode

## Context

`i` and `a` bound only behind a verb: objects could name a span for an operator
to eat, never a span for the selection to take. `viw` did nothing.
`adr/2026-08-visual-selection-is-the-anchor.md` deferred this explicitly.

## Decision

- **`i` and `a` in visual arm `Prefix::Object`**, exactly as they do behind a
  verb; `finish_object` branches on the mode rather than on whether an operator
  is armed.
- **The resolved span goes into the selection, not into a verb**:
  `Act::Place(span.start)` then `Act::Extend(head)` — the same pair `gv` uses,
  and for the same reason (the selection *is* the anchor, so there is nothing
  else to set).
- **The head rests on the span's last cluster** under `v`, which is how visual
  mode already draws its head; under `V` the raw end is enough, because a
  line-wise selection redraws itself from its ends' rows
  (`adr/2026-08-v-highlight-covers-whole-lines.md`).
- **The mode stays visual**, so `viw` then `a«` widens, as vim does.
- An object that names nothing — `i(` with no bracket — consumes its keys and
  leaves the selection standing; an unknown object key aborts the prefix, as
  everywhere else.

## Rejected

- **Repeated `iw` growing the selection** (vim's real behaviour on a second
  press) — a separate rule with its own state, for a gesture nobody in this
  vault has reached for. `v` then a motion covers the same ground.
- **Binding `i`/`a` in visual to insert-at-selection-edges** (visual block's
  `I`/`A`) — visual block is not in this editor
  (`adr/2026-08-v2-caret-first-order.md`), so the keys are free for the objects.
