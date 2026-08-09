# Delete is a palette command over the open sheet

## Context

Phase 4 extends `adr/2026-07-delete-unconfirmed-no-trash.md` to permanent notes: unconfirmed, no trash.
The gesture needs a home; the design has no buttons and destruction should not be one slip away.

## Decision

- **"delete note" is palette-only** — no chord.
  A destructive action earns a deliberate summon-and-name, not a keystroke a wrong modifier can trigger; the palette's `Context` gains `sheet_open` and the command is hidden unless a sheet is open (hidden beats disabled).
- The command deletes the sheet's file, then: closes the sheet **without flushing** (a flush would rewrite the just-deleted file from the buffer), drops the position (`Positions::remove`, `adr/2026-08-position-dropped-on-delete.md`), removes the card optimistically, and hands the editor back to the logs' selection.
- A delete the filesystem refuses keeps the sheet open with the error on its notice line — nothing is half-deleted.
- The watcher converges the index; any dangling links the deletion causes surface in the open-loops list, as designed — visible debt, not a blocker.

## Rejected

- **A confirmation step** — already rejected for time notes; notes are plain files under version-controllable dirs, and the friction system makes damage visible instead of preventing gestures.
- **A delete chord** — muscle-memory adjacency to real chords makes an unconfirmed destructive keystroke a hazard for no speed gain that matters at delete's frequency.
- **Routing through `close_sheet`** — its flush guard exists to save the buffer, which is exactly wrong here.
