# A wrapped item hangs under its own text

## Context

A block is one physical line (`adr/2026-08-per-line-block-segmentation.md`), but a long one still wraps inside its own box, and every wrapped row started flush at the item's left padding.
A checklist line long enough to wrap — the daily note's `- [ ] …` is the one that wraps most — read as two unrelated lines: the second row began under the circle, level with the marker column rather than with the text.
Plain prose has no marker and nothing to hang under, so this is about list and checklist items only; an indented prose line keeps wrapping flush.

## Decision

**The hang is two declarations on `.mk-item` and four values of one custom property.**
`padding-left` carries `--mk-hang` on top of the base indent it already carried, and `text-indent` pulls the first line back out of it by exactly the same length.
The first row therefore starts precisely where it started before — an item that does not wrap moves by nothing, and `text-indent` never touches a line box's height, so the one-line-tall rhythm holds (`adr/2026-09-every-line-shares-the-quote-rhythm.md`).
Every wrapped row starts at the padding edge, level with the text after the marker.

**`--mk-hang` is written in `em` from the prose face's own advances.**
CSS has no way to ask the layout how wide the drawn prefix is, and the prose face is proportional, so `ch` is the advance of `0` and of nothing else here.
The five glyph advances a prefix is built from — space, hyphen, bracket, bullet, circle — are read off `CormorantGaramond-Regular.ttf` (unitsPerEm 1000) and written as `em`, which on these blocks is `var(--prose-size)`: the hang follows the settings overlay's size knob for free.

**Four prefixes, because two states draw two kinds of item.**
The active and the selected block show the source, `- ` and `- [ ] `; `.block-css` hides those spans and draws `•  ` and `○ ` in their place (`adr/2026-09-inactive-blocks-hide-their-syntax.md`).
A nested item adds its own leading spaces to all four: `caret::INDENT` is two spaces per level and they carry `Role::Text`, so both states paint them, and `--mk-lead` adds `var(--mk-indent) * 2` spaces to every hang.
Each drawn prefix ends in exactly one space: a space in generated content is collapsible whatever `white-space` the block inherits, so `content: "\2022  "` renders as `• `, and `content: "\25CB "`'s own space folds into the one space of source after `]` — that byte is `Role::Text`, not `Role::Checkbox`, so `font-size: 0` never reaches it and it is the space that survives.

**The four values were measured in the running window, not only computed.**
The app was opened under the e2e harness's headless X server on a note holding one item of each kind, and the first row's text origin and the wrapped row's origin were read off the screenshot a pixel column at a time: 314 and 314 for the inactive list item, 322 and 322 for the inactive checklist, 338 and 339 for a standalone deep item active, 332 and 333 for a checklist active.
The first draft was four pixels wide on both inactive prefixes; the arithmetic said two spaces and the layout drew one, and only the screenshot could say so.

## Rejected

- **A single fixed hang** (`24px`, one step of the design's 4px grid) — the four prefixes span 0.56em to 1.57em, so one value is visibly wrong for three of them, and the whole point is landing on the text.
- **`text-indent: … hanging`** — the keyword only says *which* lines are indented; the length is still the thing CSS cannot compute, so it buys nothing over a negative indent.
- **Giving the marker span a fixed-width inline box** so the hang would be exact by construction — it works for `.block-css`, whose prefix is `::before` content this stylesheet authors, but the active block's prefix is the literal source, and padding it out to a round width would print `- ` and then a gap the user never typed. The line under the caret is the source.
- **Measuring the prefix in the webview and handing the width back as a custom property** — a third injected script, a round trip per block, and a reflow, for a constant the font file already states.
- **Hiding a nested item's leading spaces** so the hang would not need `--mk-lead` — a real simplification, and a change to `markup::model`, not to CSS; it belongs to its own decision.

## Consequences

- The four `em` constants are the one thing a prose-face change invalidates. They are named one glyph at a time in `assets/theme.css` beside the rule, and a wrong face shows up as a misaligned wrap, never as a broken line box.
- An enum item (`1. `) and a term item (`/ `) carry `mk-item` too and hang by the list item's width, which is not their own — the same `ponytail:` note that already stands over the bullet's `::before`: no note in the vault writes either.
- An item whose marker is followed by extra spaces (`-   text`, which `markup::model` folds into one marker span) hangs by the plain `- ` width. It wraps under the marker's gap rather than under the text; the source is unusual enough that measuring it is not worth a fifth rule.
- A parent item and the indented child written under it are one block, not two — `blocks::segment` splits on the newlines `typst_syntax` reports as root-level whitespace, and a nested `ListItem` is a child of its parent's node, so no newline inside the pair is ever seen. `--mk-hang` is one value per block, like `--mk-indent` already is, so the child's rows hang by the parent's prefix and land a couple of spaces off their own. The parent's rows, the only ones the reader is following, land exactly.
- The hit probe and the `j`/`k` line walk read real rects from the DOM (`launch::HIT_PROBE`, `launch::LINE_WALK`), never a computed column, so a row that starts further right is a row they measure correctly with no change.
- Nothing in Rust changed. The test that guards this reads `assets/theme.css` itself, the way the picker's sticky box is already guarded (`adr/2026-08-theme-css-inlined.md`), and asserts the padding/indent pair and the cascade order the four values depend on.
