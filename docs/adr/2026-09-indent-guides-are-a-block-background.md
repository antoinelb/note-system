# Indent guides are one background per block, and a blank line borrows its neighbours'

## Context

One indentation level in a note is two spaces (`caret::INDENT`, `adr/2026-08-tab-indents-in-every-mode.md`), and nothing on screen said so.
A nested list, a wrapped indented run, a checklist under a task heading: the depth was only ever readable by counting the gap before the text, in a proportional prose face where two spaces are 8.4px wide.
indent-blankline.nvim's answer — a thin rule standing in the column each level starts at — is the picture asked for here.

## Decision

**Guides only, no scope highlight.** A 1px rule at each level of the line's own indent, in `--guide-ink`, a divider colour a step under the quote's rule.
A line with no leading spaces draws none.

**A blank line takes the shallower of its two non-blank neighbours' depths.**
This is indent-blankline's own rule and it is the whole reason the count cannot be computed per block: a whitespace-only line has no indent of its own, and the depth that reads correctly is the one that keeps a nesting run's rules unbroken across a gap without drawing rules into open space.
A blank at either end of the note, or beside a top-level line, therefore draws none.
`blocks::guide_depths(&[&str]) -> Vec<usize>` is the one pure function that says so, called once per render over the whole block list.
Odd leading spaces round down; a tab is never an indent, because the editor only ever writes two spaces.

**One style value per block, one CSS rule per branch.**
`src/ui.rs`'s `block_style` folds the depth into the same inline `style` attribute a nested item's `--mk-indent` already rode, as `--guides: N`, and `assets/theme.css` paints them as a `repeating-linear-gradient` background on the block box — `background-size: calc(var(--guides, 0) * var(--indent-w)) 100%`, so a line with no `--guides` at all paints nothing and its DOM is exactly what it was.
The rule carries all four branches a slot can draw (`.block-active`, `.block-selected`, `.block-css`, `.block-svg`, which `.block-pending` always accompanies), because a caret move only ever swaps one block's branch for another and a guide appearing or vanishing on that move is the interface moving on its own (AIR LAY-1).
It sits below `.block-active`, whose `background: transparent` shorthand also sets `background-image` and ties with it on specificity.

**The guides ride the shared 8px gutter, not the block's content edge**: `background-origin: border-box; background-position: 8px 0`.
`.mk-item` overrides `padding-left` with its nesting step *and* its hanging indent (`adr/2026-09-wrapped-items-hang-under-their-text.md`), so guides measured from the content box would sit in a different column on a list item than on the prose line above it.
`.block-svg` carries no gutter of its own, and the same 8px puts its guides in that same column.

**`--indent-w: calc(var(--prose-size) * 0.468)`** — two spaces of the prose face, twice the `--mk-space: 0.234em` the item hang already reads off `CormorantGaramond-Regular.ttf` (unitsPerEm 1000, space advance 234), re-read from the font file for this change.
It multiplies `--prose-size` rather than being written in `em` so that it also holds on `.block-pending`'s outer div, which carries no font of its own, and still follows the settings overlay's size knob.
A prose-face change is the one thing that invalidates it, exactly as for the hang.

## Rejected

- **Scope highlighting** (indent-blankline's second half: the level containing the caret drawn brighter). It is a second colour, a second per-render computation, and it moves as the caret moves — the caret's own line already shows its source, which is the app's way of saying "you are here".
- **Guides on non-blank lines only.** Cheap — one depth per block, no whole-note pass — but it cuts every run of nesting at each blank line, which is exactly the gap the guides exist to carry the eye across.
- **A DOM element per guide.** It would let each rule be positioned exactly, but it puts N spans inside every block that draw no source bytes, and the CSS spans tiling a block's source byte for byte is what the hit probe's `data-start` walk and the tiling property both read (`adr/2026-09-property-tests-guard-three-invariants.md`). A background paints in the same place and changes nothing in the tree.
- **`ch` units for the step.** `ch` is the advance of `0`, and the prose face is proportional — the same reason `--mk-hang` is written from the font's own advances.

## Consequences

- A multi-line block (a raw fence, the folded preamble) is one line here and takes its first line's depth, the same way it takes one of everything else the block box carries.
- A guide under a compiled-fallback block (`.block-svg`) sits in the right column but against typst's own layout, which CSS did not place; it is a presence guarantee, not a pixel one.
- An indented heading draws its spaces at the heading's font size while the guides step at `--prose-size`, so the two disagree. No note in the vault indents a heading.
- On a list item the guides stand in the prose columns, left of the item's own `--mk-indent` step; that is the point of a guide column and the reason the positioning area is the border box.
