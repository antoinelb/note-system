# Visual selection is drawn across every line it covers, not just the active one

Date: 2026-08-25

## Context

adr/2026-08-visual-selection-is-the-anchor.md got the grammar right — `v`/`V` ride the phase-0
anchor, motions extend the note-global `head`, and operators already act on the true span through
`Editor::splice` — but named the drawing itself as friction: the widget only ever renders source for
the active block, so a selection crossing into a neighbouring block showed a highlight on one line
and nothing on the rest. `block_panes` (adr/2026-08-cursor-split-rendering.md) folds every block but
the active one into a single compiled Typst region per side, and a compiled SVG has no pixels to
paint a mid-block highlight over. Todo 15 asked for the fix.

## Decision

- **The drawing follows `Editor::selection()`, the note-global span; the editing unit stays the
  single active line.** No new selection state, no change to what `d`/`c`/`y` act on — only what
  `block_panes` chooses to mount.
- **A block the selection reaches but does not own becomes `Pane::Selected`** instead of folding into
  a compiled region: raw source, `.sel` spans over the covered bytes, no caret, no textarea socket.
  `block_panes` finds the boundary block on each side with `blocks::block_at` and splits the
  above/below span there — the compiled region shrinks to what the selection still leaves untouched,
  or disappears entirely when the run starts at the note's top or ends at its bottom.
- **A new pure helper, `caret::layout_selected`, draws one line's own pieces** — `Piece::Text` and
  `Piece::Selected` only, the same model `layout`'s `build_line` uses for the active block, with no
  caret/box/preview to draw. It takes the line's own block-relative byte offset as a `base` so every
  `Piece::start` stays block-relative like every other one the widget draws, even though the line
  itself is sliced out and addressed from zero.
- **`V`'s line-wise widening reaches every covered block, not only the active one.** `block_panes`
  widens `Editor::selection()` itself to whole lines — the same `motions::linewise_span` cut
  `visual_span` (vim.rs) already uses to size the operator's own span — before splitting the
  above/below run, so a boundary block's `Selected` pane and `d`/`y`/`c`'s span agree pixel for pixel.
  `v` passes the selection through unwidened and stays byte-exact, as the anchor ADR decided.
- **The compiled regions need no new eviction path.** A block drawn as `Selected` this frame is
  simply left out of the region's own source string, so it plays no part in that region's
  content-addressed key — the two-region cache (adr/2026-08-cursor-split-rendering.md) never even
  sees it, let alone needs to evict it.

## Alternatives rejected

- **Leaving a covered block's own highlight byte-exact under `V`, matching only `v`'s policy** — the
  first cut of this ADR, reverted once todo 15's own review named the mismatch: a highlight that
  stops mid-line while `d`/`y`/`c` already take the whole line reads as a rendering bug, not as a
  second, deliberate highlighting policy.
- **A second, dedicated highlight layer over the compiled SVGs** — the compiled region has no
  byte-addressable geometry to paint over; the only accurate substrate for a byte-range highlight is
  the raw source the active block already draws, so a covered block temporarily borrows it.
