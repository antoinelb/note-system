# Arrange cluster: a palette command over the open sheet, 50 iterations flat

## Context

Phase 8's on-demand layout: at most a command, force-directed never the default (plan.md § Canvas); the scope, gesture and iteration bound were open.

## Decision

- **"arrange cluster" is palette-only** (no chord) and exists only while a sheet is open: the scope is the connected component — over the undirected link graph — of the sheet's card.
  The open sheet is the one unambiguous anchor the UI already has; no selection model is invented.
- Only component members that have cards are arranged; dangling ids in the component arrange nothing.
- **The layout is a deterministic spring pass**: seeded from the cards' current resolved positions, springs along edges toward a 240px rest length, inverse-square repulsion between all pairs, per-step displacement clamped to 48px, and a hard cap of **50 iterations in a for loop** — then it stops wherever it is.
  No randomness: coincident seeds separate along an index-derived direction.
  Constants frozen here: `SPRING_LENGTH 240`, `SPRING_K 0.06`, `REPULSION 48_000`, `MAX_STEP 48`, `ARRANGE_ITERATIONS 50`.
- **Arrange may move hand-placed cards** — it is the user's explicit command, the one sanctioned mover besides the drag; it writes through the store in one batch, so the repaint and the debounced save are the existing machinery.

## Rejected

- **A chord** — layout is rare and deliberate; "at most a command".
- **Radial rings around the anchor** — trivially bounded but discards the cluster's existing arrangement, which the seed-from-current-positions pass preserves in outline.
- **Iterating to convergence** — the bound must be structural; 50 clamped steps settle small clusters and merely improve large ones, both acceptable outcomes of an explicit command.
