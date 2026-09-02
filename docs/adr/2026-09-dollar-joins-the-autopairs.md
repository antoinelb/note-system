# `$` joins the autopairs; `*` and `_` still do not

## Context

`adr/2026-08-autopairs-in-the-typing-path.md` chose the pair set `( [ { « ' " \`` — the surround set minus `*`, `_` and `<` `>`, because emphasis markers and comparison brackets punctuate prose far more often than they nest.
`$` was in neither table.
Course notes are where the equations are, and every `$x$` was two dollars typed by hand with the second one forgotten as often as a closing bracket: an unclosed `$` swallows the rest of the line into math and the block's Typst fallback shows the error until the twin arrives.

## Decision

**`$` is the eighth entry of `editor::PAIRS`**, `("$", "$", "$", false)`: typing it inserts `$$` with the caret between, typing the closing `$` steps over the one waiting, and one Backspace between them removes both.

**It carries no apostrophe guard.** The guard exists because `'` beside a word is an apostrophe and `"` before a word is closing a quote; `$` beside a word is a formula being typed (`x$` is not a thing a French sentence says — currency is written `5 $`, with the space). So `quoting` is `false` and `x` then `$` gives `x$$`.

**`*` and `_` stay out**, for the reason the earlier ADR gave: they fire mid-word in nearly every sentence. `$` is the one delimiter from the excluded half of the surround set that never appears in prose except to open math.

## Alternatives rejected

- **Pairing `$` only in a `.typ` block whose parse says math is welcome** — the typing path has no parse-tree verdict and should not need one; the pair costs one keystroke to undo when wrong.
- **A guard against a digit before `$`** (`5$`) — the vault writes currency French-style with a space; a guard for a form it never types is code with no reader.
