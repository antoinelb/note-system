# Quote blocks space at 0.9em, the per-line look's own gap

## Context

The quote show rule in `template.typ` carried `above: 0pt, below: 0pt` since quotes became full-width blocks.
Under per-line fragments that was right: each quote compiled alone in its own fragment, the pane's stacking supplied the gaps, and any internal spacing would have doubled them.
`adr/2026-08-cursor-split-rendering.md` changed the game: consecutive quote lines now compile together inside one region, so the zero override made adjacent quotes sit flush — and typst's default text edges run cap-height to baseline, so a flush quote's descenders visibly collide with the next quote's caps.

## Decision

Quote blocks take `above: 0.9em, below: 0.9em`.
That is the gap the per-line model always showed: each fragment page carried 6pt top and bottom margins, 12pt between stacked quotes, which is 0.89em at the default 13.5pt body — so the familiar rhythm survives the rendering model change.
Measured on a real nine-quote daily note: 523pt flush (overlapping), 810pt at typst's 1.2em default, 623pt at 0.9em.

## Rejected

- **Keeping 0pt** — it only ever encoded a per-line-fragment constraint that no longer exists, and it renders as overlap, not tightness.
- **Typst's default spacing (1.2em)** — tried first (2026-08-26) and judged too airy in use; paragraph-level separation reads as gaps, not as a quote list.
- **Matching `par(leading)` exactly (0.75em)** — indistinguishable from a wrapped line inside one quote; the extra 0.15em is what keeps separate quotes legible as separate.
