# `[[id]]` replaces `#l("id")`, and typing `[[` summons the picker

## Context

`#l("id")` was chosen when every note had to compile to a PDF through the vanilla CLI: a real function call the template defined, parsed as a `FuncCall` and never mistaken for prose.
Compilation still happens — export, the table's card bodies, every equation block, `make check-vault` — but the editor draws prose with CSS now (`adr/2026-08-css-draws-the-markup.md`), and the link is typed and read far more often than it is compiled.
Obsidian's `[[` is the muscle memory the user carries; `#l("` is six keystrokes of punctuation before the id.

Two facts, both probed against typst 0.15.1, decided the shape:

- `[[luhmann]]` compiles unchanged in markup mode: the lexer emits each bracket as its own `Text` leaf, so the run is literal text, exactly as `> ` is (`adr/2026-08-greater-than-is-the-stored-quote.md`).
- The same run inside backticks is one `Text` leaf under `Raw`, after `//` it is inside a `LineComment`, and the `[body]` of a `#link` is a `ContentBlock` — so reading the run off the tree keeps the "a real parse, never regex" rule: a link is a `[` `[` leaf pair, one or more text leaves that are not brackets, and a `]` `]` pair, all siblings.

## Decision

- **`[[id]]` is the one note link.** `parse::wiki_links` finds the runs among a node's children; the index (`parse_note`), the CSS model (`markup::push_children`) and the caret (`links::link_at`) all read that one function, so the three cannot disagree about what a link is.
  The id is the concatenation of the leaves between the brackets: `a_b` lexes as `a` and `_b`, `x y` stays one leaf, and both spell the id they show. `[[]]` and `[[ ]]` are no link.
- **The brackets are delimiters.** The two pairs are `Role::Link` spans flagged `delimiter`, so `.block-css .mk-delim` already hides them on an inactive block and a resting line reads `luhmann` in the link colour; the line under the caret shows `[[luhmann]]` (`adr/2026-09-inactive-blocks-hide-their-syntax.md`).
- **Vanilla Typst colours the id and drops the brackets** through a `show regex` rule inside the template's `note` function, next to the quote rule. The rule also fires inside raw text, where the app never indexes one; named as a ceiling in the template.
- **Typing `[[` opens the picker.** The autopairs already turn the second `[` into `[[]]`; the typing path recognises that exact empty pair around the caret, splices it back out, and calls the same `open_picker` Ctrl+L does. Accepting writes `[[id]]` through the one `format_link`, whichever way the picker opened; Escape leaves the prose as it was, with no `[[]]` litter. Ctrl+L stays.
  This supersedes the "trigger character" rejection in `adr/2026-08-ctrl-l-link-picker.md`: that rejection was about reading an uncontrolled textarea's text to know when the trigger fired, and the caret is app state now (`adr/2026-08-caret-on-editor-note-bytes.md`).
- **`#link` is untouched** (`adr/2026-09-link-is-for-resources.md`); the `#`-sibling retry in `link_at` now fires only from a `#`, so the byte before a `[[` stays outside the link.
- **No compatibility path for `#l`.** The template no longer defines `l`; an old `#l("x")` is an unknown call, drawn by the compiler as an error until retyped by hand, the way a note still typed `course` reads as unknown (`adr/2026-09-a-course-is-a-project.md`). The user's vault held two, migrated by `sed` with this change.
- **The truncation flag's one read-ahead**: a `[[id]]` among a visited node's own children is read when that node is, so a top-level link past the node cap is still seen; a nested one is not. The doc on `ParsedNote::truncated` says so.

## Alternatives rejected

- **Keeping `#l` beside `[[`** — two spellings of one thing means two shapes in every test and every ADR forever, for two links in the vault.
- **A regex over the source in the parser** — matches inside raw blocks, strings and comments, exactly what `link-traps.typ` exists to catch; the tree already separates those for free.
- **Leaving the empty `[[]]` in the buffer while the picker is up** — Escape would leave litter that Backspace removes in two strokes, and accept would need a second path that fills the pair rather than writing the link.
- **A show rule that skips raw** — `show raw` cannot exempt text from a later `show regex`; the mismatch is invisible in the editor and rare in a PDF.
