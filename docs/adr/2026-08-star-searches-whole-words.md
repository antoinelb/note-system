# * searches whole words, and whole-word-ness rides the pattern

## Context

`adr/2026-08-search-lands-through-place.md` gave the note `/`, `n` and `N`
over a plain substring with smartcase. `*` — search the word under the caret —
was never added.

`*` that matches substrings is not `*`: pressing it on `mot` and landing
inside `motif` is precisely the behaviour the key exists to avoid.

## Decision

- **The committed pattern stops being a bare `String`.** `motions::Pattern`
  carries the text *and* `whole_word`. `/` builds `Pattern::loose`; `*` and `#`
  build one with `whole_word: true`.
- **Whole-word-ness rides the pattern, not the keystroke**, so `n` and `N`
  after a `*` keep walking whole words — which is what vim does and what makes
  the pair of keys usable together. A later `/` replaces the pattern and its
  flag together.
- **A hit is whole-word when neither neighbour is a word character.** The line
  slice is the whole context: a match never straddles a line
  (`adr/2026-08-search-lands-through-place.md`), so the line's own ends bound a
  word as surely as a space does.
- **`*` takes the word the caret is on, or the next one along its line.** vim
  scans forward for a *keyword* character rather than taking whatever run the
  caret happens to stand in, so `*` on a space or on a comma still finds the
  word after it. With no word left on the line the key is consumed and the
  caret stays.
- Smartcase is unchanged and shared: `motions::smartcase` is now one function
  that `/`, `*` and the ex line's `:s` all call, so the three cannot drift on
  what counts as a match.

## Rejected

- **A plain substring `*`** — cheaper by one struct field, and wrong in the one
  case the key is for.
- **A separate `whole_word` flag on `Vim` beside the pattern** — two fields that
  must be set and cleared together are one field waiting to disagree.
- **Regex word boundaries** — no regex crate enters the codebase for this
  (`adr/2026-08-ex-line-is-literal-and-global.md` makes the same call), and the
  character-class check is four lines against the `Class` the word motions
  already use.
