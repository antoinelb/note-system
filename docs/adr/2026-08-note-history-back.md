# Ctrl+B: a back history over logs selections and sheets

## Context

The rail, the crumbs, time-navigation and link-following all move the
selection or open a sheet, but none of them remember where you came from.
Obsidian's Ctrl+O gives exactly that — the last few things you had open,
one keystroke back through them. Todo 22 asks for the same gesture under
Ctrl+B (Ctrl+O already belongs to jump-to-note,
`adr/2026-08-jump-ctrl-o-centres-viewport.md`).

## Decision

**A single bounded stack, `Signal<Vec<Visit>>`, pushed to by exactly two
openers and popped by one chord.**

- `enum Visit { Logs(Selection), Sheet(String) }` — a visit is either "the
  logs pane was showing this selection" or "this sheet was open".
- **What counts as a visit**: `select` and `show_sheet` push what stood
  immediately before they change anything — a sheet's id if one was open
  at the moment they ran, otherwise the logs pane's current selection.
  This is a structural fact (`sheet.peek()`), not a judgement about *why*
  the opener ran, so it also covers the cases the two obvious call sites
  don't first close a sheet for: the palette's time-navigation commands
  and a sheet-to-sheet link follow both reach `select`/`show_sheet`
  directly, sheet still open, and correctly leave that sheet as the visit
  behind.
- **What deliberately does not push**: `close_sheet` (a return to the
  selection already standing, not a new place), `take_disk` (a reload of
  the same note, external-conflict resolution), and `edit_template` (a
  side door into the one editor, not a note visit). Pushing on these would
  teach Ctrl+B to walk back into places nothing ever navigated away from.
  The Escape handler that leaves template editing (`ui.rs`'s `keyboard`
  closure, `Key::Escape if over_template`) is the same case reached from a
  different door: it calls `select` with the selection already standing,
  a return rather than a new visit, so it sets `restoring_history` around
  that call the same way `go_back` does below — without it, `select`'s
  push is unconditional and cannot tell "returning from the template side
  door" from "navigating to a new note".
- **`restoring_history: Signal<bool>`, set only around a landing call**:
  `go_back` sets it before calling `select`/`open_sheet` to land on the
  visit it just popped, and the template-Escape return above sets it
  around its own `select` call, both clearing it immediately after.
  `select` and `show_sheet` skip their push while it reads true. This is
  what makes the stack an actual walk rather than a two-item swap — see
  below.
- **The cap**: a plain `HISTORY_CAP = 64`, oldest entry dropped first
  (`push_visit`). Unbounded would leak for the length of a session;
  Obsidian's own list is a similar small handful — depth is a convenience
  bound, not a designed number.
- **Ctrl+B pops one entry**: `Visit::Logs(sel)` runs `go_logs` then
  `select(sel)`; `Visit::Sheet(id)` runs `go_table` then `open_sheet(id)` —
  the same seams every other opener uses, so the flush guard and the
  screen hygiene both still apply. **An empty stack is a silent no-op** —
  there is nowhere behind the first note, and Ctrl+B is not a command that
  earns a notice for finding nothing to do.
- Wired at pane level in both keydown closures, guarded like every other
  overlay-aware chord (`adr/2026-08-screen-switch-gesture.md`'s
  precedent), plus a `CommandId::Back` palette row (chord `ctrl+b`,
  chordless commands keep the `_ => true` default visibility — there is no
  "history is empty" context flag, matching the chord's own silent no-op).

Without `restoring_history`, landing on a popped visit would itself look
like a new visit to `select`/`show_sheet`, which would push the
just-departed place straight back onto the stack — one press would land
on the previous visit, but a second press right after would only return
to where the first one started, an oscillating swap rather than a walk,
with everything earlier in the stack permanently unreachable. Suppressing
the push for exactly the landing call is what keeps repeated Ctrl+B a
genuine walk: pressing it three times after visiting A, B, C in order (in
that order, current at D) lands on C, then B, then A in turn, each press
consuming one more entry, same as Obsidian's Ctrl+O.

## Rejected

- **Separate back/forward stacks** — a real browser-history model, but
  nothing else in the app tracks a forward direction, and the ask was one
  chord, not a navigation pair.
- **Pushing from every place `screen`/`sheet`/`selected` changes** —
  `close_sheet`, `take_disk` and `edit_template` all touch that state
  without the user having gone anywhere new; treating them as visits would
  make Ctrl+B bounce between a note and its own template or conflict
  reload.
- **Hiding "back" in the palette while the stack is empty**, mirroring
  `undo`'s `undoable` flag — the chord itself is deliberately a silent
  no-op rather than a guarded one, so the row stays consistent with it and
  needs no `Context` field the other eight navigation commands don't
  already lack.
