# Neighbours yield along the line between centres, the further card first

## Context

`adr/2026-09-cards-yield-on-drop.md` resolves overlap on drop: the dropped card holds, every card within 8px of another slides clear, in sweeps over every pair in id order.
Two of its rules put cards in places no hand would have put them:

- **The slide ran along the shallower axis.** A card is 176 wide and 56 tall, so any neighbour less than about 120 to the side of the drop found its vertical escape shorter and was sent *down*, however clearly it stood to the right.
- **Between two free cards the higher id yielded.** A card the drop had just pushed outward, meeting a lower id further out, was pushed back *toward* the drop, then out again by the drop, and so on until the 32-pass cap ran out — a ricochet the notice then reported as crowding.

The user's word for the wanted behaviour is magnets: the dropped card repels its neighbours, and they in turn repel theirs, outward.

## Decision

- **A push runs along the line through the two centres, away from the card that holds**, by the shortest run that clears one axis: the offset scaled by `min(depth_x / |dx|, depth_y / |dy|)`.
  The axis that clears is written exactly rather than through the factor, so the pair reads as clear on the next pass and no rounding residue can spend the cap.
  A zero offset on one axis never clears that axis — the division yields the infinity that loses the min — and two coincident cards, with no line between them, still separate downward.
- **Between two free cards, the one further from the nearest dropped card yields**; a tie yields the higher id, so the same drop still resolves the same way on every run.
  An anchor never yields and a pair of anchors is still skipped.
  With one anchor, pushing the further card away from the nearer one strictly increases its distance from the drop, so the disturbance propagates outward and never turns back.
- **Everything else stands**: the 8px gap, the 56-tall box at every scale, the id-ordered sweep, the 32-pass cap, the crowded notice, the one `positions` write and the undo rules.
  The cap is still reached — a card wedged dead on the column between two members of a dropped set has no line to slide out along — and the notice still says so.

## Rejected

- **Simultaneous force accumulation** (every free card sums its pushes, all applied at once, each pass).
  The most literal magnet model, but two pushes at an angle partly undo each other, and splitting a push between two free cards converges geometrically — the reason `cards-yield-on-drop` already refused halving: no pass ever separates nothing and every crowded drop reports crowding.
  Sequential exact pushes keep the resolver's verdict sound.
- **Keeping the shallower axis and only fixing the yield order.** The ricochet was one of the two complaints; a neighbour to the right sent down was the other.
- **Live repulsion during the drag.** Still refused for the reason `cards-yield-on-drop` gives: motion under the cursor the user did not cause.
