# A note reopens where it was left

## Context

Every note opened, however often, put the caret in the same place: `Editor::open` woke the last block with the caret at the end of the text (`adr/2026-08-cursor-always-in-the-note.md`).
For a day note that is the bottom of a growing log, which is roughly right by accident; for a permanent note reached twice a day it means scrolling back to the paragraph under work every single time, and the caret-centring ADR (`adr/2026-09-the-caret-line-sits-at-the-centre.md`) made that scroll conspicuous rather than fixing it.

Two questions had to be answered before code: where the memory lives, and what a note nobody has opened does.

## Decision

**A plain-lines file at `vault/.index/carets`, the positions file's other sibling.**
Where the user stopped reading is user data with no upstream — nothing in the notes derives it, and a rebuilt index cannot reconstruct it.
That is the property `2026-07-positions-separate-file.md` used to keep positions out of the database and `2026-09-palette-orders-by-usage.md` reused for the usage counts, and it applies here unchanged.
`src/carets.rs` is `src/usage.rs`'s shape line for line: one entry per line, whitespace-separated, no extension on the filename; loaded once at mount; a missing or unreadable file is "nothing remembered"; a malformed line loses that line, not the file; unknown keys ride along and survive a save; every save is write-temp-then-rename through `persist::write_atomic` with the parent ensured first.
A refused write reports as its own `Source::Carets`, a warning rather than a critical — the note itself reached disk and no prose is at stake, only the memory of a place.

**The key is the note's vault-relative path, not its id.** `permanent/luhmann.typ 12 4`.
An id would be the tidier key and is wrong: a note whose `#meta` is missing or broken has no id — that is exactly the debt the loops list names and the loops-list ADR already had to route around (`adr/2026-09-loop-lines-open-their-notes.md`, "some of what it names has no id row for the index to resolve") — and it deserves to reopen where it was left like any other.
A path this format cannot spell (empty, or carrying whitespace the reader splits on) is not remembered rather than written as a line the next load would drop.

**The value is a line and a column, not a byte offset.** The file may have been edited outside the app between the two visits: a line number clamps to the note's last line and a column, counted in chars of that line, clamps to that line's length, where a stale byte offset would land mid-word or off a char boundary. `carets::locate` and `carets::place` are that pair, pure and unit-tested; `place` never panics and never lands off a boundary.

**A note nobody has opened lands at the end of its title heading** — the first line starting with `= `, past the `#import`/`#meta` preamble every note carries.
Failing that, the end of the first line with anything on it; failing that, the start of the note.
`carets::first_open` is that rule, pure and tested: the caret arrives ready to write the note's first sentence rather than inside its metadata.

**Remembered when the note is left, and on each autosave tick.**
`ui::swap_editor` is the one seam that replaces the open note — the rail and Ctrl+D through `select`, a sheet opening through `show_sheet`, the sheet closing, a template opened, a conflict's take-disk — so it remembers the outgoing note's caret and lands the arriving one, and the six call sites each changed by one line.
The autosave resource remembers on its own 500 ms debounce, so a session that ends without ever leaving the note loses at most that debounce; the Ctrl+Q flush remembers too.
Nothing is written per keystroke.
**Deleting a note drops its entry**, positions' drop-on-delete mirrored, so a note recreated at the same path opens like one nobody has opened.

**The landing is `Editor::land_at_open`, not `place_at`.** `place_at` flushes on its way through `activate`, and a buffer read from disk this instant has nothing to save: routing every note open through it would bump the mtime of every note the user merely looks at and wake the watcher for each one.

## Consequences

Every note now opens with its title heading awake instead of its trailing empty line, which is a visible change to the one state every headless test starts from: the block a click used to wake is awake already and carries no click listener.
`ui::tests::activate_heading` is therefore gone, replaced by `woken_targets`, which reads the block the mount recorded — the same targets, one gesture fewer.
A note with no `= ` line at all (a fixture whose heading was replaced by an equation) opens on the end of its `#import` line, the fallback's honest answer.

## Alternatives rejected

- **A session-only map, like the theme and the font size** (`adr/2026-08-settings-overlay.md`) — those are two knobs with a defensible default on every launch. A memory of where you were reading that resets when you close the app forgets exactly when it would have paid off, which is the same argument that rejected session-only usage counts.
- **A column in the SQLite index** — one fewer file, but the index is deliberately disposable (`2026-07-disposable-index-user-version.md`) and the exception would have to be remembered by every future rebuild. The failure the positions ADR made structural instead of remembered.
- **Keying by id** — see above: a typeless note has no id, and the notes that most need forgiveness are the ones already carrying debt. The path is also what the delete path and the loops list already hold.
- **A byte offset instead of line and column** — cheaper to compute and unsafe across an external edit; the clamp would have to guess, and guessing lands mid-word.
- **Landing a first-time open at the top of the note** — that is the `#import` line, which nobody wants to edit, and the preamble is four to eight lines the caret would have to walk past on every new note.
- **Writing per keystroke** — a file write per key, for a value only read when a note opens.

## Ceiling

Undoing a delete (`adr/2026-08-app-level-undo-register.md`) restores the note's text and its card's coordinates, not its caret: the restored note opens at its title.
The intent would have to carry a third field for a case measured in one keystroke of scrolling.

## Verified

`make static` clean, `make check-vault` clean, `make test` at 100% regions, lines and functions (1229 lib tests, 93 integration).
`tests/e2e/caret-memory.test.sh` passes, and so do `daily-note`, `create-note`, `visual-line-j-k`, `ex-line-substitutes`, `open-note-switcher`, `wiki-link-trigger`, `search-text` and `delete-then-undo` — the scenarios that type into a note the caret no longer opens at the end of.
