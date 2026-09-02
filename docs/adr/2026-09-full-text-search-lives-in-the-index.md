# Full-text search is an FTS5 table in the index, opened by Ctrl+Shift+F

## Context

Notes were findable by id and title (the Ctrl+L picker, the Ctrl+B recent list, the table's filter) and never by what they say.
A student looking for "the lecture where the professor mentioned Banach" has a word, not an id.
The index is SQLite, rebuilt from the files whenever its schema changes (`adr/2026-07-disposable-index-user-version.md`), and rusqlite's bundled SQLite carries FTS5.

## Decision

**`notes_fts` is an FTS5 virtual table beside `notes`** — path (unindexed), title, body — written by the same `insert_note` every other table is, emptied by the same rebuild, and deleted by hand on `delete_note` because a virtual table has no foreign key.
`Note` now carries its `source`, the text the parser read, so the index never re-reads a file.
Schema version 5: the index rebuilds itself on the first launch.

**The body starts past the preamble.** Every template opens with `#import`, `#show` and `#meta` and closes that run with a blank line, so a note whose first line is an import is indexed from its first blank line on; `template` and `meta` hit only notes that actually say them.

**The query is words, not syntax.** Each whitespace-separated word becomes one quoted FTS5 term with inner quotes doubled, joined by the implicit AND; a `*` or an `AND` a student types is text the tokenizer folds, never an operator that errors.
`unicode61 remove_diacritics 2` folds accents both ways, so `idee` finds `idée`.
At most twelve hits, best rank first; an empty query finds nothing.

**Ctrl+Shift+F opens the finder** on either screen, and the palette row "search text" runs the same thing.
It is the recent-notes picker's twin: a query, rows with the note's title (or stem) and a snippet, arrows, Enter, Escape, and a row click.
It searches on every keystroke — the index is local and the vault small, so the answer lands inside the keystroke.
A hit opens the way a loop line opens (`adr/2026-09-loop-lines-open-their-notes.md`): a `time/` note on the logs, anything else in a sheet.
Ctrl+F alone stays the table's card filter, which now refuses the shifted chord.

## Alternatives rejected

- **A `LIKE '%word%'` over a stored body column** — no ranking, no snippet, no diacritic folding, and a scan of every note on every keystroke.
- **Grep over the files at query time** — the index exists so the screen never waits on the disk; a search that read every `.typ` on each keystroke would be the one feature that did.
- **FTS5 query syntax passed through** — power for the one user who knows it, a syntax error on the status line for every `"` in a French sentence.
- **Indexing the preamble too** — every note would hit `template`, `meta`, `show` and the note's own id, which the pickers already find.
