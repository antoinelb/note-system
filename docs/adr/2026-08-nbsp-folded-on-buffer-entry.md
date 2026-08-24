# The no-break space folds on the way into the buffer

## Context

Some `- [ ]` items in the daily note rendered as a task circle and others as a plain `•` bullet followed by the literal text `[ ] …`, with no visible difference between them in the editor.

The cause is the keyboard, not the renderer.
On the `ca` (Canadian Multilingual) layout the bracket and the no-break space share the AltGr level — `key <AD11> {[dead_circumflex, dead_circumflex, bracketleft]}` and `key <SPCE> {[space, space, nobreakspace]}` — so typing `[`, space, `]` without releasing AltGr between them writes U+00A0 where a space was meant.
`keymap::action` keeps Alt-modified keys insertable on purpose, so that `« » € œ` type at all (`adr/2026-07-checklist-rendering.md` is what the stray byte then defeats), and the app wrote the no-break space faithfully to disk.

Typst gives the item body the same five children either way — `[`, the middle, `]`, a space, the text — but the middle child's identity differs: a plain space is a `space` element with no fields, while U+00A0 is a `text` element whose `text` field holds `"\u{a0}"`.
The checklist rule matches the middle with `c.at(1) == [ ]`, which is true for the first and false for the second, so the item fell through to the default `list.item` and printed its brackets.

## Decision

- **U+00A0 folds to a plain space at `Editor`'s two text-entry points**, `insert_at_caret` and `paste`, through the shared `nbsp_folded`. The buffer holds no no-break space, so `- [ ]` is always plain ASCII on disk and the existing template rule matches every time.
- **`insert_at_caret` is the whole typing story.** Vim's insert mode is phase 0's writing flow untouched, returning `Outcome::Pass` into `keymap::action`, and vim's own `Act::Type` replay for `s`, `R`, `o` and the dot lands in the same method — one fold covers every typed route in both modes.
- **The fold runs before the caret arithmetic, not after the splice.** `insert_at_caret` places the caret at `content.start + span.start + text.len()` and `paste` derives its span and landing offset from the clip through `motions::paste_spec`; U+00A0 is two bytes against the space's one, so folding late would leave the caret a byte right of the text per stray space.
- **Paste folds too**, on both routes — `insert_at_caret` for the plain paste and `paste` for vim's `p`/`P` — so the invariant is statable without exceptions: no edit puts a no-break space in the buffer.
- **U+00A0 alone.** It is the only such character the `ca` layout can produce.
- **`template.typ` is unchanged.** The rule still matches a `space` element only.

Notes already carrying the byte are not reached by a fold on new edits; the four items in `time/2026-08-24.typ` were repaired in the same change, and `LC_ALL=C grep -rn --include='*.typ' $'\xc2\xa0'` over the vault is the check that no others remain.

## Rejected

- **Widening the template's match to accept U+00A0** — renders correctly and fixes existing notes retroactively, but leaves an invisible byte on disk that every later consumer has to re-learn: a grep for `- [ ]`, a `[ ]`→`[x]` toggle, the vanilla typst CLI under another template. The file should say what was meant.
- **Folding in `keymap::action`** — the natural home, since the comment there already reasons about AltGr, but it sees only the `Key::Character` path and would miss vim's `Act::Type` replay.
- **Folding every Unicode space separator** (U+202F, U+2007, U+2009 …) — no evidence any of them can reach the buffer, and a wider rewrite rule than the cause warrants.
- **Folding at the widget or the save** — the widget is one of several entry points and a save-time fold would let the wrong bytes drive rendering for the whole editing session.
