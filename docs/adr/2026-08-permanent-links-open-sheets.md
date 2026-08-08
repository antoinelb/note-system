# Links to table notes open sheets

## Context

v0 left two branches deliberately inert: a footer entry whose target is not
a time note rendered unclickable, and Ctrl+Enter over such a target did
nothing (`adr/2026-08-ctrl-enter-opens-time-links.md`, "time notes only";
`adr/2026-07-permanent-notes-wait-for-table.md`).
Phase 3 gives those targets somewhere to open: the writing sheet.

## Decision

- **The "time notes only" clause ends.** `follow_at` gains one arm: a target
  that is no time note but sits in the table's notes opens its card's sheet
  — switching to the table if the chord fired from the logs.
  Ctrl+Enter, Ctrl+click in the source, and the palette's "follow link" all
  ride the same callback, on both screens; the table pane's keydown gains
  the Ctrl+Enter and Ctrl+L arms the logs pane already had.
- **A time link followed from a sheet lands on the logs**: the sheet closes
  (flushing), the screen switches, and the existing `select` does the rest
  — the one editor is free to hold the day.
- **The footer's inert arm splits on reachability.** A non-dangling entry
  whose target sits on the table becomes a `link-jump` that opens the
  sheet.
  Dangling links stay inert (debt lives in the loops list), and so does a
  backlink labelled by the stem of an id-less note — positions and cards
  are keyed by id, so no card can host it.
- **A lookup that fails still opens the sheet**, as a closed editor carrying
  the error on the notice line — the message appears where the user is
  looking, and Escape closes it.
  The refusal case is the other direction: if the *current* buffer cannot
  flush, the sheet does not open at all
  (`adr/2026-08-sheet-reuses-the-one-editor.md`).

## Rejected

- **A new chord or palette command for "open in sheet"** — following a link
  is one gesture; where the target lives decides what opens, not which key
  was pressed.
- **Keeping capture/generated targets inert** — the sheet takes every card
  kind (`adr/2026-08-every-card-opens-the-sheet.md`); the footer follows
  the table.
- **Making the stem-labelled backlink clickable into an error sheet** — the
  only sheet it could open is a notice saying the id does not exist;
  showing the entry inert is the honest rendering of "the table cannot host
  this".
