# Enter is the way back into a rendered note

## Context

`adr/2026-08-shift-escape-leaves-the-note.md` made leaving a note a
deliberate gesture — and left no keyboard way back in. Once shift+Escape had
rendered the block, the only returns were a mouse click on a block or a rail
selection that reopens the note from scratch.

On the logs pane, Enter already meant "the selected time note": it creates
the file when the day, week or season has none. Over a note that already
exists it did nothing at all — an early `return`.

## Decision

- **Enter over a note that already stands puts the caret back into it**,
  through the new `Editor::reactivate`: the block owning the remembered
  caret wakes and the caret lands exactly where deactivation left it. Not
  the last block, which is where `Editor::open` starts a fresh note — the
  caret survives on `Editor` while nothing is active, so returning is
  literally returning.
- **The create arm is untouched**: only Enter over a missing note writes a
  file, and navigating still never does.
- **The mode is not reset.** `Vim::note_opened` is for a *new* note reaching
  the editor; re-entering the one already open is a fresh activation, and
  those never reset the mode
  (`adr/2026-08-escape-ladder-editor-wide-mode.md`).
- **No new key, no palette entry.** Enter on the pane already meant "act on
  the selected note"; this is that same meaning over the other half of the
  state, and it is not a chord
  (`adr/2026-08-caret-shape-is-the-mode-indicator.md` § the palette
  boundary).
- **The pane's Enter only ever fires with no block active** — an active
  block owns Enter in both modes, so the arm needs no guard of its own.

The table screen needs no twin: shift+Escape closes the sheet there, so a
note is never left standing rendered with the caret put away.

## Rejected

- **`Editor::open` on the selected path again** — it rereads the file, resets
  the undo history and lands on the last block. Returning is not reopening.
- **A dedicated key (`i`, or Ctrl+Enter)** — `i` belongs to the grammar and
  means nothing while the pane holds focus; Ctrl+Enter already follows the
  link under the caret (`adr/2026-08-ctrl-enter-opens-time-links.md`). Enter
  was already the pane's "act on this note" key and had a dead branch.
