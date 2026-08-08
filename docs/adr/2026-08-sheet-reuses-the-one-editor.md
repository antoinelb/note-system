# The writing sheet reuses the one editor

## Context

Phase 3 mounts the hybrid block editor inside the writing sheet (wireframe
state 6b).
The app has exactly one `Signal<Editor>`, and everything the roadmap says
"must simply keep holding" — the debounced autosave resource, the single
`QuitFlush` callback, the notice line, the watcher's
never-reload-the-open-buffer rule — subscribes to that one signal.
The two screens never show their editors at the same time.

## Decision

- **Opening a sheet loads the note into the existing editor signal**
  (`open_sheet` → `open_sheet_note` → `editor.set`); closing it restores the
  logs' selection through the existing `open_selected`.
  Autosave, Ctrl+Q flush and the notice line work in the sheet with zero new
  wiring.
- **Invariant: a sheet is open ⇒ the editor holds that note ⇒ the screen is
  the table.** `go_logs` closes the sheet first, exactly as `go_table`
  closes the active block and picker — no overlay waits behind a screen.
- **The buffer reaches disk before it is replaced.** Both `open_sheet` and
  `close_sheet` guard on `editor.write().flush()`: a save that fails keeps
  the current note open, its error on the notice line, rather than dropping
  the text with the editor.
  This is stricter than the logs' own `select` (which leaves pending edits
  to the autosave) because `select` swaps between time notes the rail still
  lists, while a refused sheet has no other surface to show the loss on.
- **The widget is shared, not duplicated**: the block-pane rsx and the link
  picker rsx became local closures in `Shell` (`blocks_view`,
  `picker_view`), called from the logs centre pane and from the sheet.
  Buffer/widget separation is untouched — the closures still only forward
  events to `Editor`.

## Rejected

- **A second `Signal<Editor>` for the sheet** — a second autosave resource,
  a two-buffer quit flush, a second notice surface and a doubled test
  matrix, to preserve a logs buffer that `open_selected` rebuilds from disk
  in one call.
- **Extracting the block panes into a component** — Dioxus props must be
  owned + `Clone` + `PartialEq`; the widget would need eight of them
  (editor, fragments, probe, writer, epoch, pending caret, …) for no reuse
  beyond these two call sites.
