# % matches vim's default pairs, plus the guillemets, and ignores its count

## Context

The user runs `vim-matchup` and `showmatch = true`; `%` is a live habit that
this editor had no answer for. Typst notes are full of brackets — `#l("…")`,
`#table(…)`, `#meta(…)` — and the multi-line ones are exactly where jumping
to the partner by hand is worst.

## Decision

- **The pair set is `()`, `[]`, `{}` — vim's own default `matchpairs` — plus
  `«»`**, because the notes are French and the guillemets are already a
  first-class pair everywhere else in the grammar
  (`adr/2026-08-surround-pair-set-and-padding.md`).
- **`<>` stays out**, even though the surround table carries it. In prose a
  bare `<` is a comparison far more often than a Typst label, and there is no
  parse to tell the two apart at the point `%` runs. A `%` that lands
  somewhere wrong is worse than one that does nothing — the second is a
  no-op, the first moves the caret into a place the user then has to notice.
- **The bracket is found on the caret's line, the partner note-wide.** vim's
  rule for finding the bracket is "the first one at or after the cursor on
  this line"; the search for its partner then counts nesting of that same
  pair across the whole note, so a multi-line `#table(…)` — one block by
  `adr/2026-08-per-line-block-segmentation.md` — matches across every line it
  spans.
- **`%` is inclusive under an operator**, so `d%` takes the closing bracket
  with it, as vim's does.
- **A count is spent and ignored.** vim's `[count]%` means "jump to N% of the
  file", which a note has no use for. Ignoring it is better than letting `2%`
  do something surprising.
- Same-character quotes are not pairs, as in vim: `"` is handled by the quote
  objects, which pair left to right along a line.

## Rejected

- **Including `<>`** — see above; a Typst `<label>` is real, but so is `a < b`,
  and the false match is the more expensive error.
- **Deferring to the `typst-syntax` parse to find the true pair** — correct in
  principle, and the never-regex rule points that way for *extraction*. But `%`
  runs on every keystroke over a buffer that is mid-edit and frequently
  unparseable; a nesting count over the raw text answers on an unbalanced
  document, where a parse tree does not.
- **Matching quotes with `%`** — vim does not, and the quote objects already do.
