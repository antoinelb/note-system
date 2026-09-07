# Neighbours yield while the card is in flight

## Context

`adr/2026-09-cards-yield-on-drop.md` resolved overlap on the drop and nowhere else, and rejected "live physics: cards repel continuously while the drag is in flight" for two reasons: motion under the cursor the user did not cause, and a debounced store write per frame.
In use the drop came as a surprise: the dragged card slid over its neighbours as if they were paper, and only on release did they jump clear.
The user asked for the neighbours to move while the card is being dragged.

## Decision

- **Every move of a drag previews the drop's own resolution.**
  After the dragged set's store write, the move handler runs the same `table::resolve_group` the drop will run — the set at its current place, every other card where the store has it — and puts the pushed coordinates in a `pushed` signal.
  `table::previewed` draws the cards with those coordinates over the store's; the edges follow, since they are derived from the drawn cards.
- **A preview is not a write.** Nothing of the neighbours reaches `positions` until the release; the debounced write a drag already restarts on every move carries only the dragged set, as before.
- **The drop lands exactly what the hand was shown.** `settle_cards` calls the same `resolve_landing` off the same store state, so the persisted pushes equal the last preview; it clears the signal, as does every release, so a click's sub-slop wobble leaves no ghost.
- **A hand that retreats gets the neighbours back.** The preview is a function of the pointer, computed from the pre-drag layout each time, never accumulated: a card pushed aside returns when the dragged card leaves.
  This is the one place the behaviour diverges from real magnets, and it is what keeps the earlier ADR's rule true — nothing on the table *stays* moved that the user did not just move.
- The crowded notice stays a drop-time verdict: the preview shows what the passes managed and says nothing.

## Rejected

- **Accumulating pushes across the drag** (each move resolving from the previous move's pushed layout).
  Physically truer, but a wandering drag would plough a trail through the constellation, and a drag gains no undo (`adr/2026-08-app-level-undo-register.md`).
- **Writing the previewed pushes to the store on every move.** The per-frame write the earlier ADR refused; the preview costs one resolve per move and no I/O.
- **Animating the neighbours into place on the drop.** Motion the user still cannot steer, and a second source of truth for where a card is during the animation.
