# The cursor always lives somewhere in an open note

> Superseded in part by `adr/2026-08-escape-ladder-editor-wide-mode.md` and
> `adr/2026-08-shift-escape-leaves-the-note.md`: "Escape never deactivates"
> below no longer holds as stated. A plain Escape is now inert in normal
> mode and Shift+Escape is the one gesture that puts the cursor away and,
> on the table, closes the sheet. Everything else stands: a note opens with
> its last block active, the default caret is the block's end, and the last
> block keeps its trailing newline visible as the line writing starts on.

## Context

Since v0's hybrid editor, opening a note activated nothing: every block rendered, the caret existed only after a click, and Escape returned to the caretless state.
The request: while a note is shown, the cursor should always be somewhere — by default on the last line, with a new note offering an empty line under its title to start writing on.

## Decision

- **An open note always has an active block.**
  `Editor::open` wakes with the *last* block active; every open path — the logs selection, the sheet, creation — inherits it.
  A closed editor (no note) still has no cursor, which is what lets plain Enter create the selected time note.
- **Escape never deactivates.**
  The block lets the keystroke bubble: over the sheet the pane closes it — one press instead of two — and on the logs it reaches the loops-list arm or nothing.
  A one-block note therefore stays in source until another block is activated; the rendered view of any block is one click on a neighbour away.
  The screen switch stops deactivating too: the cursor survives a logs → table → logs round trip.
- **The default caret is the end of the block.**
  A textarea mounting without a pending caret (accepted completion, overlay restore) puts the caret at its end — the note's last line — instead of wherever the webview lands it.
- **The note's last block keeps its ending visible.**
  Segmentation's "no phantom blank lines" rule gets one exception: the final block's content runs to the end of the note, so the trailing newline every template already ends with *is* the empty line under the title where the cursor rests and writing begins.
  Interior separators stay hidden as before; templates need no change.
- Conceded knowingly: ←/→ no longer page the month while a note is open (the caret owns the arrows); the wheel, the header buttons and the palette remain.

## Rejected

- **Cursor placed only at open, Escape keeping its deactivate role** — "always" would be false one keystroke in; the Obsidian instinct this app replaces never loses its cursor.
- **A trailing empty *block* from a template parbreak** — segmentation folds trailing breaks into the last block by design; fighting that with template `\n\n` would just show two blank lines.
- **Making all blocks show their separators** — the phantom-blank-lines rule is right everywhere except at the note's end, where the blankness is the point.
