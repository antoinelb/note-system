# "edit template" reaches the logs from the table

## Context

`adr/2026-08-template-editing-in-the-one-editor.md` hid the palette's **edit template** row on the table: the logs' centre pane is the only full-page surface the one shared editor has off the table, and that ADR rejected generalizing the sheet to hold paths.
Hiding it was the cheapest way to keep that promise, but it is the one row the registry withholds for a reason that is not the user's.
Every other hidden command is either a place you already stand (`go to table`), a construct only one screen owns (the zooms, the finders, the folds), or a thing the context genuinely lacks (a sheet, a conflict, an undo).
Reshaping what every future note looks like is none of those: it belongs to neither screen, and a vocabulary that shrinks when you cross one is a mode the user has to remember.

## Decision

The row stands on every screen.
The picker it opens — the jump overlay's grammar over a `read_dir` of `templates/` — is now rendered inside the table branch as well as the logs branch, the way the finders already live inside the table branch, so its chords bubble to whichever pane hosts it and the focus-relay rule (`adr/2026-09-overlay-keys-relay-before-focus-lands.md`) reaches it from either.
A screen switch closes it in both directions now, `go_logs` mirroring `go_table`, and the table pane's chord arms gained the `template_picker` guard the logs' already carried, so overlays still never stack.

Choosing a template still opens it in the logs' centre pane: `edit_template` ends with `screen.set(Screen::Logs)`, a no-op where the pane already stands.
The order is the table's own Ctrl+D (`open_daily` before `go_logs`):

1. the flush guard first — a refused flush keeps the picker open and the note in place, unchanged;
2. then a sheet's `select`-style bookkeeping — the sheet goes onto Ctrl+B's visit log, its picker and card close;
3. then the editor takes the template, and only then does the screen change.

Not `go_logs` itself: its `close_sheet` would put the logs' selected note back into the one editor the template is about to take.
The sheet is pushed onto the visit log even though `edit_template` pushes nothing on the logs — there, Escape out of a template returns to the selection it replaced, so a push would duplicate it; from a sheet, Escape lands on the logs selection instead, and the visit log is the only way back to the card.

## Alternatives rejected

- **A template sheet on the table** — the sheet is keyed by note id through the index and templates have no index entry; the prior ADR already refused to widen that seam, and nothing here changes the arithmetic.
- **Keeping the command logs-only** — it teaches that the vocabulary shrinks when you cross a screen, for a command whose subject (the shape of every future note) belongs to neither screen. The one-keystroke workaround (Ctrl+2, then the palette) is exactly what the palette exists to spare.
- **Floating the picker at the top level, beside the palette and the notices overlay** — it renders on both screens for free, but the overlay stops being a descendant of either pane, and Ctrl+1 typed at its input no longer bubbles to a screen-switch arm. The finders' placement (inside the branch, floating by CSS) already solves this and keeps the chords.
