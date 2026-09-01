# W B E ge gE arrive as new variants, not as a flag on the old ones

## Context

`adr/2026-08-motions-on-visible-lines.md` gave the note `w b e` and never
mentioned their WORD siblings — not a rejection, an omission. The user's own
nvim runs `camelcasemotion`, which rebinds `w b e` to sub-word motions and
leaves `W B E` alone; the capitals are therefore the keys their fingers reach
for when they want to cross a whole `l'idée,` in one press.

`ge` and `gE` were missing for the same reason.

## Decision

- **A WORD is a word with punctuation folded in, and nothing else.**
  `motions::class` already answered `Word | Punct | Blank`; it takes a `big`
  flag, and `big` makes `Punct` read as `Word`. The three scanners
  (`word_forward`, `word_back`, `word_end`) and `word_object` take the same
  flag. There is no second scanner.
- **Five new `Motion` variants, rather than parameterising the existing three.**
  `BigWordForward`, `BigWordBack`, `BigWordEnd`, `WordEndBack`, `BigWordEndBack`.
  `Motion` is spelled out in ~40 test literals and inside
  `Change::Operate { noun: Noun::Motion(..) }`; adding variants costs nothing,
  while changing `WordForward` into `Word(Big)` would have rewritten every one
  of them for no behavioural gain.
- **`ObjectKind::BigWord`** gives `iW` / `aW`, sharing `word_object`.
- **`ge`/`gE` are spelled as the predicate, not as `word_end`'s mirror.**
  A word end is any non-blank whose following character belongs to another
  class (or ends the note); the landing is the nearest such position behind
  the caret. Written that way, the "the caret is still inside its own run"
  case — which the mirror form needs an extra branch for — simply does not
  arise.
- **Span kinds** follow their small siblings: `W`/`B` exclusive, `E`/`ge`/`gE`
  inclusive.
- **Both house quirks are mirrored**: `cW` acts as `cE` on a WORD, and `dW` on
  a line's last WORD stops at the line's end — the same two exceptions
  `adr/2026-08-one-register-the-clipboard.md` recorded for `cw`/`dw`. A quirk
  that held for one size and not the other would be worse than not having the
  key.

## Rejected

- **Parameterising `Motion::Word*` with a `Big` marker** — the same behaviour
  at the cost of rewriting every existing test literal and the dot's recorded
  nouns.
- **A separate WORD scanner** — the difference is one character class; two
  scanners would be two places for the block-separator and French-letter rules
  to drift apart.
- **Rebinding `w b e` to camelCase sub-words**, as the user's nvim does — that
  plugin exists for source code with `camelCaseIdentifiers`. Notes are French
  prose; `l'idée` splitting at the apostrophe is already the useful default
  (`adr/2026-08-motions-on-visible-lines.md`).
