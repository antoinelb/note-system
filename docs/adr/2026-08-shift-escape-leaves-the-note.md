# Shift+Escape leaves the note; plain Escape never does

## Context

`adr/2026-08-escape-ladder-editor-wide-mode.md` gave Escape three rungs, the
third of which throws you out of the note: normal-mode Escape returned
`Act::Deactivate`, the block rendered again, and the focus effect handed
focus from the IME sink back to the pane.

But Escape in vim is also the key you press *to be sure of the mode* — a
reflex, pressed constantly and meaning nothing. One extra press past "stop
typing" and the caret was gone from the note. The free gesture and the
destructive one were the same key.

## Decision

- **A plain Escape can never leave a note or drop focus.** In normal mode it
  is swallowed: it kills the pending grammar as before, and then goes inert.
  Because the sink stops propagation on a swallow, it cannot reach the pane
  either — no sheet closes, no overlay closes, no notice is acknowledged
  while the caret is in the writing.
- **Shift+Escape is the one way out, from any mode.** `Vim::handle`
  intercepts it before the mode dispatch, runs the mode's own Escape first —
  insert's caret step-back and dot capture, R's single clipboard write,
  visual's landing at the head — then appends `Act::Deactivate`. The session
  closes exactly as it always did; only the departure is new.
- **On the table, Shift+Escape takes the sheet with it.** It is the one
  keystroke in the app that produces acts *without* stopping propagation, so
  it bubbles to the table pane, which closes the sheet and puts the card
  back. The sheet is the note there; leaving the block but staying in the
  sheet is a state nobody wants.
- **Both panes guard their Escape arms with `!shift`** — the logs pane's arm
  is empty on purpose. A note leaving on Shift+Escape must not fall down the
  plain-Escape ladder and acknowledge a critical on its way out.
- **No palette entry**: modal keys are a grammar, not commands
  (`adr/2026-08-caret-shape-is-the-mode-indicator.md` § the palette boundary).

This supersedes the third rung of
`adr/2026-08-escape-ladder-editor-wide-mode.md`. The first two rungs stand
unchanged: insert → normal, and pending grammar dies first.

The way back in is `adr/2026-08-enter-returns-to-the-note.md`.

## Rejected

- **Shift+Escape only from normal mode**, leaving insert two keystrokes from
  the exit. The gesture exists because leaving should be asked for, not
  because it should be laborious; one key out of anywhere is the point.
- **Deactivate only, leaving the sheet open** — the same keystroke count as
  before the change, and it strands you in a sheet with nothing in it.
- **Keeping Escape as the exit and adding a confirmation** — friction the
  house style forbids (`plan.md` § no hard blocks), and it would still fire
  on the reflex press.
