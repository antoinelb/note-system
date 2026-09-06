# Cards never overlap: the neighbours yield on drop

## Context

Nothing stopped two cards from covering each other.
A drag wrote its position live and released wherever the hand stopped; a new card landed on the viewport centre whether or not something already stood there (`adr/2026-08-new-card-lands-at-viewport-centre.md`); the arrange's spring pass laid its cluster out with no regard for the cards around it (`adr/2026-08-arrange-cluster-command.md`).
A buried card is unreachable — the one on top takes every click — and the constellation stops being a map.

Two placements were already clear by construction and stay that way: the ring walk's cell pitch is 192 × 96 and the origin grid's is the same, both wider than the 184 × 64 a card needs, and `table::cards` feeds every position already resolved into the ring's `occupied` list.
What had no rule was the three places a card *lands*.

## Decision

Taken with the user (2026-09-06):

- **Resolution happens on drop, never continuously.**
  Three seams call one callback, `ui::settle_cards`: a drag released beyond the click slop, a Ctrl+N creation landing on the viewport centre, and an arrange laying its cluster down.
  A click that opened a sheet moves nothing; a re-render moves nothing; nothing on the table moves that the user did not just move.
- **The dropped card is the anchor and the neighbours yield.**
  The card stays exactly where the hand left it, and every card whose rectangle comes within the gap of another slides clear along its **shallower axis** — the shorter of the two escapes, so a card dropped on a row slides its neighbour sideways and a card dropped on a column slides it down.
  A card pushed may push the next: the resolution is a full sweep over every pair, repeated.
- **8px of clear canvas between card rectangles** (`table::CARD_GAP`), the design's unit like every other gap.
- **The card box is the titles-zoom one — 176 × 56 canvas units** (`CARD_WIDTH` × `CARD_HEIGHT`, twice the tether's drop, the one card height the layout declares) — **whatever zoom the drop happened at**.
  Body zoom's cards are 296 tall in the same canvas units and overlap by construction: the origin grid's own row pitch is 96 and the ring's `SLOT_H` is 96, so every placement rule in the file is already a titles-zoom rule.
  Spreading a constellation by 304 units to clear a card the user is only reading would wreck the map the titles view is.
- **The pushes are deterministic.**
  The working set is sorted by id; a pass walks every pair in that order; the anchor never yields, and between two ordinary neighbours the higher id does.
  The same drop over the same table pushes the same cards to the same coordinates on every run, so a test can pin them.
- **The passes are capped at 32** (`table::RESOLVE_PASSES`), with an early stop the first time a pass separates nothing.
  A card wedged between the anchor and a card that outranks it can be slid off one and onto the other forever; the cap is what ends that.
  When the cap runs out, **what the passes managed stands and the status line says so** — `layout: too tight to clear every card …`, a warning, resolved by the next drop that comes out clear.
  The drop itself is never refused: crowding is visible debt like every other kind, never a save-blocker.
- **Moved neighbours persist through the seam positions already use.** One `positions.with_mut` writes the anchor's landing and every push together — one repaint, one debounce, one atomic write of the plain-lines file (`adr/2026-07-positions-separate-file.md`, `adr/2026-08-atomic-persist-seam.md`).
  A pushed card that had no entry gains one: a card that yielded has been put somewhere on purpose, so it stops drifting with its links and is pinned like a dragged one.
- **Undo follows the register as it stands** (`adr/2026-08-app-level-undo-register.md`): an arrange's pushes join its `Intent::Arrange` before-image, keyed by id so a card the spring pass moved *and* the resolution pushed reverses to the coordinates it had before either — one undo takes the whole landing back.
  A drag and a creation gain no undo, because that ADR decided drags are not undoable and a creation is not a destruction.
  Nothing is added here.

## Rejected

- **Snap-clear: the dropped card bounces to the nearest free spot.**
  It contradicts the gesture — the user put the card *there* — and a drop near a crowded region would fling it somewhere it was never aimed.
- **Live physics: cards repel continuously while the drag is in flight.**
  Motion under the cursor that the user did not cause, sixty times a second, on top of a debounced store write per frame.
  The table is a map, not a simulation; the arrange is the one place a force pass belongs, and it is explicit, bounded and palette-summoned.
- **Resolving at the current zoom's card height.**
  The same layout would be "clean" or "dirty" depending on what the user was looking at, and one drop at body zoom would spread the whole constellation.
- **Refusing a drop the passes cannot clear.**
  A hard block, against the standing rule that all friction is visible debt.
- **Discarding the partial resolution when the cap runs out.**
  It would undo real separations and leave the dropped card buried; the notice tells the truth either way.
- **Halving the push between two ordinary neighbours** (each yields half the depth).
  It converges geometrically instead of exactly, so no pass ever separates nothing and every crowded drop reports crowding.
- **Anchoring a card once it has yielded**, to make the chain terminate in one pass per card.
  Two anchored cards would then skip each other, and the resolver could report a clear layout with an overlap left in it.
  A sound verdict is worth more than a guaranteed pass count.

## Out of scope, named

The origin grid still ignores the store: an unplaced, unlinked note takes its session slot near the origin whether or not a hand-placed card stands there.
That slot is a proposal, not a landing — no drop happened — and the first drop involving either card resolves it.

## Amended by `2026-09-shift-drag-selects-cards.md` (2026-09-06)

`resolve_drop` is now the one-card case of `resolve_group`, which takes a
slice of anchors: a whole dropped set holds where the hand left it, no
member ever yields, and a pair of members is skipped outright — the set is
one rigid body. Everything else is unchanged: the shallower axis, the 8px
gap, the id order between two ordinary neighbours, the 32-pass cap and the
crowded notice.
