# Ctrl+Shift+D deletes the open sheet's note immediately

## Context

`adr/2026-08-delete-note-palette-only-from-sheet.md` put delete behind the
palette on purpose: no chord, because "muscle-memory adjacency makes an
unconfirmed destructive keystroke a hazard for no speed gain that matters
at delete's frequency." Todo 27 asks for a delete chord anyway
(Ctrl+Shift+D). This ADR **supersedes** that rejection — the decision to
go trashless and confirmation-free stands, but the chord it stood against
is now in.

## What made the chord unacceptable then, and what changed

The 2026-07/08 argument was about *adjacency*: a plain chord next to the
ordinary ones a wrong finger reaches. Three things distinguish
Ctrl+Shift+D from that hazard rather than being another instance of it:

- **The sheet-open guard.** The chord is inert unless a sheet already
  stands open — exactly the palette's own `sheet_open` visibility rule
  (`adr/2026-08-delete-note-palette-only-from-sheet.md`'s "hidden beats
  disabled" idiom, read the other way: here it's "guarded beats absent").
  A slipped Ctrl+Shift+D over the table with nothing open, or from the
  logs screen, does nothing — there is no path where it deletes a note
  the user did not deliberately have raised.
- **The shift modifier.** Ctrl+D alone is the daily-note chord, reached
  for constantly. Ctrl+Shift+D is not a stray finger away from that: it is
  a second key a hand must deliberately add, the same distance AZERTY and
  QWERTY both keep between "open" and "destroy" on every other chord in
  this app (Ctrl+N vs. nothing adjacent, Ctrl+, vs. nothing adjacent). The
  2026-07 hazard was a *bare* chord; this one was never proposed bare.
- **Undo.** `adr/2026-08-app-level-undo-register.md` postdates the
  original rejection: every delete, chord or palette, now leaves a
  before-image one "undo" away. The 2026-07 ADR reasoned about
  irreversible destruction with a keystroke; that premise no longer holds
  for either delete path.

Confirmation dialogs are still not the fix — they were never the
argument, and clicking through them under stress is exactly what makes
them theatre rather than safety. The guard, the modifier and undo are the
three things that make a keystroke acceptable in their place.

## Decision

- **Ctrl+Shift+D runs the existing `delete_note` callback** — the same
  code path the palette's "delete note" row already uses (built
  `undo::Intent::Delete` before touching the file, reports
  `Notice::delete_failed` on a refusal, hands the editor back to the
  logs' selection on success). No new deletion logic; only a second way to
  trigger it.
- **Wired in `table_keys` only.** The sheet is a table-only construct —
  `show_sheet` always sets `screen.set(Screen::Table)`, and `go_logs`
  always closes any open sheet first (`adr/2026-08-screen-switch-gesture.md`).
  The sheet cannot stand open behind the logs screen, so a Ctrl+Shift+D
  arm in the logs pane's `keyboard` closure would forever read
  `sheet.peek().is_none()` and never fire — dead code the compiler cannot
  catch. It is commented at the call site rather than duplicated.
- **The plain Ctrl+D arm gained `&& !event.modifiers().shift()`** so the
  two chords cannot shadow each other regardless of match order.
- **The palette keeps its "delete note" row, chordless in the registry.**
  Like Ctrl+T (`adr/2026-08-ctrl-t-toggles-the-todo.md`), Ctrl+Shift+D
  answers to no `CommandId` — it calls `delete_note` directly, the same
  documented exception the completeness audit test already carries a
  precedent for.

## Rejected

- **A confirmation step** — see above; already rejected for time notes and
  for the palette path, and stress-tested software shows people click
  through modals, not read them.
- **Reusing `CommandId::DeleteNote`'s chord field** — the palette's
  "delete note" row would then advertise a hint that only applies with a
  sheet already open and the shift held, duplicating what the guard
  already enforces at the call site; simpler to leave the registry
  chordless and let the pane guard speak for itself, as Ctrl+T already
  does.
- **Wiring the arm into the logs pane's `keyboard` closure too**, for
  symmetry with every other chord in this round — symmetry with a branch
  that can never execute is not a virtue; the comment at the table's own
  arm says why it stands alone.
