# The gutter numbers every line from the caret

## Context

The editor is a vim layer over a block map where a block is exactly one physical line (`adr/2026-08-per-line-block-segmentation.md`), and the whole point of that grain is that `dd`, `ip`/`ap` and every motion act on one line.
`[count]j`, `[count]k`, `d5j`, `:12` — the grammar's counts are all counted in lines, and the note gave the eye nothing to count them with.
The caret's line already sits at the pane's centre (`adr/2026-09-the-caret-line-sits-at-the-centre.md`), so "the line I want is four down" is a question the screen could answer and did not; the user counted rows by hand or typed `j` four times.

## Decision

**A gutter left of every block shows one number, and the numbers are vim's `set number relativenumber`:** the caret's own line states its absolute 1-based number, every other line its distance from the caret.
Two pure functions hold the whole arithmetic — `blocks::line_label(index, caret_line)` and `blocks::gutter_width(blocks)` — and they live beside `segment`, because a line *is* a block here and nothing else needs to know it.

**It is always shown**: normal, visual and insert mode alike, on the logs editor and on the table's sheet card, both of which mount the one `blocks_view` closure (`adr/2026-08-sheet-reuses-the-one-editor.md`).
No toggle and no setting: the settings overlay holds the two knobs that move the type and the theme (`adr/2026-08-settings-overlay.md`) and gains nothing by holding a third for a column that costs 20px.

**Every one of the five slots draws it**, the compiled fallback's two included (`Pane::Fragment`, `Pane::Pending`), or the numbering would skip exactly the lines an equation or a `#table` sits on and read as a miscount rather than as a rendering detail.
The number rides *inside* its slot, so the click that already activates a block covers its number too — no second handler, and no way for the two to disagree about which line was clicked.

**Nothing about the block box changes.** `.line-number` is `position: absolute` against the slot, with `right: calc(100% + 4px)` putting its right edge 4px short of the slot's own left edge — which right-aligns the column for free, at any digit count, with no width to state.
The 4px is air between the digits and a quote's rule, which paints on that very edge: flush, the two read as one glyph (the first day of use showed it), and the reservation on `.note-blocks` carries the same 4px so the prose moves by nothing.
The 8px between the digits and the prose is the block box's own horizontal gutter, not a second one, so the shared box (`adr/2026-09-every-line-shares-the-quote-rhythm.md`) and `.mk-item`'s hanging indent (`adr/2026-09-wrapped-items-hang-under-their-text.md`) are untouched by construction: an out-of-flow element cannot move them.
`line-height` is one prose line box, so a wrapped block keeps its number on its first row.

**The column's width is the note's, never the caret's.** `blocks::gutter_width` is the widest absolute number the note can ever show — `max(2, digits)` — written once on `div.note-blocks` as `--line-digits` and reserved there as `padding-left`.
Typing a line that carries a note past 9 or past 99 therefore widens nothing and moves no prose (AIR LAY-1); the alternative, measuring per line, would shift the whole column sideways mid-keystroke.
The reservation is spelled from the label face's own digit advance (DejaVu Sans Mono, 1233/2048 em), the way `--mk-hang`'s four values already are.

**The compiled fallback's scroll box moves from the slot to the widget.** `.block-svg`'s `overflow-x: auto` existed so a wide compiled block scrolls inside itself instead of forcing the reading column wider; left on the slot it would also clip the number, which sits outside the slot's left edge. It now sits on `.block-svg .note` and `.block-svg .render-error`, which scroll identically.

**Two properties the slot leaks into the number, closed after the first day of use.** `text-indent` is inherited, and `.mk-item` hangs its first row by a negative one (`adr/2026-09-wrapped-items-hang-under-their-text.md`): the number inherited it, its shrink-to-fit box collapsed to nothing, and its digits were drawn that far left of the column on every list line — 9.7px on a bullet, 18px on a checklist, most on an active `- [x]` line whose hang is the whole source prefix. `.line-number` now states `text-indent: 0`. The quote's rule was a `border-left`, and a border sits outside the padding box an absolutely positioned child measures `right: 100%` from, so a quote's number stood 1px right of its neighbours'; the rule is now an inset `box-shadow` of the same 1px at the same edge, which moves no box. Both were measured in a browser against the shipped stylesheet, not argued from the spec.

**The unit is the physical line, not the block, and the number rides in the source line.** A block is one physical line almost always, but the parse tree keeps a nested list, a raw fence and the folded preamble whole (`adr/2026-08-per-line-block-segmentation.md`), and `j`/`k` walk the lines inside them one at a time. Numbering blocks left every nested item blank and put the caret's absolute number beside its parent, rows above the caret. `blocks::line_of` names the line a byte falls on; each of the three source-drawing slots emits the number as the first child of every `.source-line`, where its static position — no `top` — is that line's own row, wrapped or not; a compiled fallback has no source lines and wears one number, its first line's, the lines it hides still counting. `.source-line` stays unpositioned so the slot remains the containing block.

## Alternatives rejected

- **Normal mode only.** The counts the numbers serve are a normal-mode grammar, so this is the tempting cut — but a column that appears and disappears with the mode is a layout change on every `i` and every Escape, and the shared block box exists precisely so that entering a line shifts nothing below it. A gutter that comes and goes would undo that at the column level.
- **A setting, in the Ctrl+, overlay.** A knob for a decision that has one right answer for a single user with no accounts. The overlay's two existing controls move type size and theme, both of which the user genuinely changes; this one would be set once and never touched.
- **Absolute numbers only.** Honest, and useless for the thing the numbers are for: `12j` needs the *distance*, and reading it off absolute numbers is the subtraction the relative column exists to do. Vim's own default pairs the two exactly this way, and the caret's line keeps its absolute number so `:N` still has something to read.
- **A number per *visual* row of a wrapped block.** A block is one line and every motion acts on one line; numbering wrapped rows would name lines the grammar cannot reach.
- **Rendering the gutter as a sibling column of the block list** (a two-column grid, numbers on one side, blocks on the other). It keeps the number entirely out of the block's own box, but it splits one line's identity across two DOM subtrees: the click that activates a block would need a second handler on the number, and the two lists would have to be kept in lockstep by index rather than by containment.
