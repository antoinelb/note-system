# Three invariants are held by property tests, not only by examples

## Context

The suite is example-based: 1,095 unit tests each state one input. The
grammar, the markup model and the note parser each promise something for
*every* input — "no key sequence corrupts the buffer", "the spans tile the
source byte for byte", "any file parses" — and no example test can say
"every". The 2026-09-02 audit listed the absence; the first run of the
tests below made the case by itself.

## Decision

**`proptest` is a dev-dependency, and `tests/integration/properties.rs`
holds three properties** at 2,000 cases each (256, proptest's default,
missed the first defect; 5,000 found it; 2,000 finds all three known ones
in under two seconds):

- **Any key sequence leaves the editor sound, and undo walks back to the
  opened text.** Keys from the grammar's whole alphabet, with the digits,
  the prompts' sigils and the platform keys, run through `Vim::handle` and
  the same act executor `src/ui.rs` uses — minus the webview and the
  clipboard — against a real `Editor` on a temp file. After every key the
  editor reports no trouble; after the sequence, `undo` until the history
  is spent restores the original.
- **CSS spans tile a block's source**: ascending, contiguous, from 0 to
  `len`, ending on char boundaries — the contract `markup::Markup`
  documents.
- **The parser survives anything**, and reports no `#meta` where the word
  is not spelled.

The first property found three defects on its first stress run, all in
byte arithmetic the examples never reached:

1. **`R` then a letter at an opened note's caret** (past the trailing
   newline of a block whose content ends with one — a lone `_` or `*`
   parses into such a block) built a splice span running backwards. Fixed
   in `motions::Lines::of`, which had stripped that trailing row on
   purpose ("no phantom row"); the row is where the caret legally rests,
   so the table now keeps it.
2. **A linewise `gu`/`gU`/`g~` over a character whose case-mate has a
   different byte length** (Ⱥ is two bytes, ⱥ three) kept the caret at
   its old byte offset, now inside a character; every later edit was
   refused as stale. The caret is measured in the recased text.
3. **`~` over the same** landed past the old span's end rather than the
   flipped run's. Same fix.

`Editor::splice`'s in-block path also floors a mid-character caret onto a
boundary now, as its cross-block path always did: a grammar mistake
degrades instead of poisoning the session.

## Alternatives rejected

- **More example tests** — each of the three defects needed a specific
  Unicode character or a malformed parse at a specific caret; the examples
  that existed were written by someone who knew the intended arithmetic.
- **Fuzzing with `cargo-fuzz`** — needs nightly and a separate crate, and
  finds crashes, not "undo restored the wrong text".
- **A property over `src/ui.rs`'s executor through a `VirtualDom`** — the
  executor reads signals and spawns futures; the harness's copy is twelve
  arms of one-line delegations, cheaper to keep than a component-level
  driver.
