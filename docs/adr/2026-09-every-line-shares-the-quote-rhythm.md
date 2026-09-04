# Every line shares the quote's rhythm

## Context

A note's list ran down the page at almost twice the pitch of the prose beside it: "all lists should take the same vertical space as regular text".

Measured in the shipped window, headless X at 1400x900, on a day note holding three prose lines, three `> ` quotes, three `- ` items, three `- [ ] ` checklist items and three `> ` quotes carrying `$x$` (an `Equation` node, off `markup::kind_is_css_safe`'s allow-list, so those three fall through to the compiled Typst widget), reading the ink rows of the bullet column and of the quote rules off the screenshot:

```
before                       row pitch
  prose alpha/beta/gamma       47, 47
  quote alpha/beta/gamma       47, 47   (rules: three 43px bands, 4px apart)
  item  alpha/beta/gamma       47, 47
  task  alpha/beta/gamma       47, 47
  fallback quotes ($x$)        28, 29   (compiled SVG, no CSS box at all)
```

47px is the 27px line box (`--prose-size: 18px` × `--prose-leading: 1.5`) plus the shared block box's `padding: 8px` top and bottom, plus the 4px that survives margin collapsing between two `margin: 4px 0` siblings — 20px of air under every line.

So the loose rhythm was never a list rule, and the tight one was never a quote rule.
`.mk-item` only ever adds `padding-left`, `.mk-quote` only a `border-left` and `padding-left`, and the `font-size: 0` marker spans of `adr/2026-09-inactive-blocks-hide-their-syntax.md` add no height at all: **every** line CSS drew carried the same 20px of air — prose, heading, item, checklist, quote and blank alike.
The one thing in a note that was tight is the compiled fallback: `.block-svg` carries no box, so its lines are the SVG's own height, edge to edge.
A run of quotes reading tight beside a run of checklist items reading loose is that seam — two pipelines drawing the same note at two rhythms — not a difference between quotes and lists.

The box itself is shared on purpose: it sits identically on the four rules that draw a block's source (`.block-active`, `.block-pending .pending-source`, `.block-selected`, `.block-css`) so that the caret entering or leaving a line cannot shift the lines below it (`adr/2026-08-css-draws-the-markup.md`).

## Decision

The shared block box loses its vertical half: `margin: 0; padding: 0 8px` on all four rules.
One physical line is now exactly one line box tall — `calc(var(--prose-size) * var(--prose-leading))` — whatever its markup role.
The horizontal 8px stays: it is the gutter the quote's rule and the item's bullet hang in, and the base `.mk-quote`'s and `.mk-item`'s own `padding-left` is measured against. Both numbers are multiples of 4 (CLAUDE.md § Design).

Measured again, same note, same display:

```
after                        row pitch
  prose alpha/beta/gamma       27, 27
  quote alpha/beta/gamma       27, 27   (rules: one continuous 81px band)
  item  alpha/beta/gamma       27, 27
  task  alpha/beta/gamma       27, 27
  fallback quotes ($x$)        28, 29   (unchanged — the SVG's own height)
```

The compiled fallback and the CSS path now agree to within a pixel and a half, which they never did before.

The invariant the file's comments insist on is untouched and easier to hold, because the box no longer has a vertical half to get wrong.
Verified by walking the caret up onto `- item beta` with `k` and diffing the ink rows against the same note at rest: only the caret's own line changes (it shows its `- ` marker and the box caret), and every band above and below it — including the three compiled fallback lines under it — sits at the identical row.

## Rejected

- **Zeroing the box on `.mk-item` alone.** It would have made lists match prose and left both at nearly twice the pitch of a compiled block beside them, which is the seam the report actually saw. The block box is one box; the fix belongs on the box.
- **Keeping the 8px vertical padding and dropping only the margin** (a 43px pitch). Still 16px of air per line, still visibly looser than the compiled fallback, and a rhythm nothing else in the note agrees with.
- **Giving `.block-svg` the same 8px vertical padding instead, so the compiled blocks read loose too.** That trades a tight note for a loose one, which is the opposite of what was asked, and it cannot be made exact anyway: the SVG's height is typst's, not CSS's.
- **Adding air back around headings only.** Legitimate as its own decision — a symmetric margin on `.block-active.mk-h*` and `.block-css.mk-h*` together would keep the caret invariant — but one rhythm for every line is the simpler thing to hold, and no note asked for it.
- **Spacing lines with `line-height` instead of a box.** The caret bar and the selection fill span a span's content box (`.caret::after`, `.sel`), so the air would end up painted as part of the line's highlight.
