# Autopairs live in the typing path, not the grammar

## Context

Every `(` `[` `{` `"` `«` in a Typst note is typed twice by hand: once to open, once to close.
`#l(`, `#meta(`, a quoted phrase, a French guillemet pair — the closing delimiter is always a second keystroke and often a forgotten one.
vim-surround (`adr/2026-08-surround-pair-set-and-padding.md`) already wraps an *existing* span; nothing closes a pair as it is being typed.

Insert mode is phase 0's writing flow (`src/vim.rs` `insert_key`), so the grammar never sees an ordinary typed character.

## Decision

- **The pair set is `( [ { « ' " \``** — the surround set minus `*`, `_` and `<` `>`.
  Typst's emphasis markers and the comparison brackets punctuate prose far more often than they nest; closing them automatically would fire on nearly every sentence.
  `«` carries the padding the surround keys already chose (`« x »`), so typing it leaves `«  »` and the content grows between the two spaces.
- **A new door, `Editor::insert_typed`, called from exactly one site** — `keymap::Action::Insert` in `src/ui.rs`.
  Paste, the IME's commit and the link picker keep going through `insert_at_caret`, which never pairs: text someone already balanced must not be balanced twice.
  This is the whole reason the behaviour is not simply added to `insert_at_caret`.
- **Three behaviours**: an opening cluster inserts both halves with the caret between them; the closing cluster steps over a close already waiting rather than doubling it (over the guillemet's padding space too); one Backspace between the two halves removes both.
- **A quote-like against a word character on either side opens nothing.**
  In a French vault `'` is an apostrophe far more often than a delimiter — `l'ami`, `d'accord`, `n'est` — and a quote typed in front of an existing word is closing one, not opening one.
  Brackets carry no such doubt and always pair.
- **A selection is replaced, never wrapped.** Wrapping a span is what visual `S` is for; a typed character over a selection keeps phase 0's meaning.
- **The table is its own**, a `PAIRS` const in `src/editor.rs`, not shared with `vim::surround_pair`.
  The two answer different questions — *what wraps a span* (including the closing keys `b`, `B`, `]`, `}` that name a pair from either end) versus *what closes as you type* — and the surround table's `*`/`_`/`<` entries are exactly the ones autopairs must not have.
- **The indent unit and the pair set both live beside the code that uses them**; `caret::INDENT` is shared with the grammar's Tab because that value genuinely must not drift (`adr/2026-08-tab-indents-in-every-mode.md`).

## Rejected

- **Pairing inside `insert_at_caret`** — would double every bracket in pasted code and in an IME commit, with no way for the caller to opt out.
- **The full surround pair set** — `*` and `_` would close on every emphasis typed mid-word, and `<` on every comparison.
- **Pairing quotes unconditionally** — `l'ami` becomes `l''ami`, which is the single most common thing typed in this vault.
- **Wrapping a selection in the typed pair** (nvim-autopairs' own behaviour) — visual `S` already does it through the grammar, with a checkpoint and a dot record; a second path would diverge.
- **Sharing `vim::surround_pair`** — would require filtering out the closing-key aliases and the three excluded characters at every call site, which is more code than the seven-line table it would save.
