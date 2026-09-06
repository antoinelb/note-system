# Shift+drag selects cards, and the picked set moves as one body

## Context

`todo.md` asked for "a way to select multiple cards to move together", and
`docs/plans/2026-09-multi-select-cards.md` laid out four designs for it:
A, a marquee plus Shift+click; B, a card cursor and a visual mode; C, sets
named from the palette; D, a marking mode with a count badge.
The plan's own recommendation was B with A folded in.

Two facts framed the choice.
On the bare table every bare key is free — `sink_keys` runs the vim
grammar only over a screen that hosts a note — so any of the four could be
built without fighting an existing gesture.
And the no-overlap rule had just landed (`adr/2026-09-cards-yield-on-drop.md`):
a card that lands holds its place and its neighbours slide clear, which
means a *group* landing needs a rule of its own, or the pass would shove
the members apart and destroy the arrangement the move was meant to
preserve.

## Decision

Taken with the user (2026-09-06): **design A, by itself.**

- **Shift+drag on the void draws a marquee**, and on release every card
  whose rectangle *intersects* it is the new selection — intersects, not
  contains, because the band is aimed by hand and a card is 176 units
  wide. A bare void drag still pans, unchanged; the marquee needed a
  modifier precisely because the pan is the table's most-used gesture.
- **A bare void click clears the selection** — travel inside
  `table::CLICK_SLOP`, the same verdict that tells a click from a drag
  (`adr/2026-08-click-opens-drag-moves.md`).
- **Shift+click on a card toggles it in or out** and opens no sheet. A
  plain click still opens the sheet and leaves the set standing, so a
  picked card can be read without being dropped.
- **Dragging any picked card drags the whole set**, every member keeping
  its offset, written live per frame through the one store write the
  single-card drag already used. **Dragging an unpicked card moves only
  it** and leaves the selection exactly as it was.
- **On a group drop the set is one rigid body.** `table::resolve_group`
  takes a slice of anchors instead of one: no member ever yields, a pair
  of members is skipped outright, and only outside cards slide clear with
  the existing `CARD_GAP`, in the same passes capped at `RESOLVE_PASSES`.
  `resolve_drop` is now its one-card case, so the creation and the arrange
  are unchanged.
- **The mark is hue *and* weight** (AIR INP-3): `.card.picked` puts
  `--selection` on the border and lays `--select-fill` over the card's own
  ground as a background *image*, so a capture's and a generated card's
  fills still show through. The band is a `div.marquee` in the same two
  tokens. Both already carry each theme. **No count badge** — the marks
  are the count, and nothing new is asked of the chrome.
- **Escape on the bare table clears a standing selection**, one rung below
  the loops overlay and one above the notice: the set goes before anything
  is acknowledged.
- **The canvas declines every press** — `.canvas { pointer-events: none }`,
  the cards taking their own back with `pointer-events: auto`. A marquee is
  an *absolute* rectangle, so it needs the client point converted to canvas
  coordinates, which needs the pane's origin, which the press itself
  reports as the difference between its client point and its pane-local
  offset. That difference is only trustworthy when the press's target is
  known, and the canvas carries a transform that would make its own offsets
  canvas-local at one zoom and pane-local at another. With the canvas out of
  the way, every void press lands on the pane and the origin is exact at
  every zoom and pan — no hardcoded chrome height anywhere.

Two decisions the user took against the plan's own text:

- **No undo for drags, single or group.** The plan argued that yielding
  neighbours had voided `adr/2026-08-app-level-undo-register.md`'s
  "the gesture corrects itself by dragging back", and proposed an
  `Intent::Move`. The user reaffirmed the rejection instead: dragging a
  group back does not un-yield the neighbours it pushed, and that is
  accepted. No `Intent::Move`, no change to `src/undo.rs`. The arrange
  keeps its own before-image, delete keeps its own, and drags stay what
  they have always been — self-correcting, cheap, and outside the register.
- **The selection dies with the table view.** Ctrl+1, Ctrl+2, the chrome
  icons and every other route to the logs drop it, and it never survives to
  the logs and back. It is session state like the pan and the zoom, but
  unlike them it is a *pending act*, and a pending act that outlives the
  screen it was assembled on is a trap. One `use_effect` over `screen`
  carries it, not a line in `go_logs`: four other callbacks set the screen
  themselves.

## Rejected

- **Design B, the card cursor and visual mode** — the largest of the four
  (a cursor, a mode, seven key arms, two ADRs, two scenarios) and the
  keyboard half is not what was asked for; the plan's own §B has the cost.
- **Design C, sets named from the palette** — ships no per-card picking at
  all, and "select filtered" can name a set larger than the viewport; plan §C.
- **Design D, a marking mode with a badge** — makes a plain click mean two
  different things depending on a mode, the failure AIR's LAY section names
  outright; plan §D.
- **The bare void drag as the marquee** — a head-on collision with the pan.
- **Contains rather than intersects** — a band would have to swallow a card
  whole to take it, which at 176 units wide means aiming, not sweeping.
- **A count on the chrome** — the marks already say how many, and the
  chrome's one line is the notice surface
  (`adr/2026-09-the-table-draws-the-notice-line.md`).
- **Adding to the selection with each marquee** — a band names a set; the
  gesture that adds one card at a time is the Shift+click, and having both
  add would leave no way to start over but Escape.
- **Excluding a picked card from opening its sheet** — a plain click on a
  picked card still opens it, because the mark is a membership, not a mode.
- **Resolving a group drop as N independent drops** — the members would
  shove each other and the move would destroy the arrangement it was
  supposed to carry; this is the rigid-body rule above.
- **Hardcoding the chrome's height** to convert a client point to the
  canvas — the header's height is a layout fact that a font metric can
  move; the press already carries the answer.

## Out of scope, named

The keyboard cannot pick a card: there is still no card cursor on the
table, so the selection is a mouse gesture from end to end. Design B in the
plan is the increment that would change that, and it is not taken here.
