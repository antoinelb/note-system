# Templates edit in the one editor, reached by a palette command

## Context

Templates are editable notes by design, but nothing in the app could open one: the editor's five open paths all derive from the logs selection or the index, and templates are never indexed (`scan_vault` walks only the category directories).
Editing a template meant leaving the app.

## Decision

One chordless palette command, **edit template** (hidden on the table — the logs' centre pane is the only full-page surface the one shared editor has off the table).
It opens a picker overlay in the jump overlay's grammar, fed by `read_dir` on `templates/` — the directory, not the index, is the authority.
Choosing a template opens it in the one editor with the sheet's flush discipline: the current buffer reaches disk first, and a refused flush keeps the picker open and the note in place.

"Template mode" is not state: it is derived from the buffer's own path (`open_template`, path under `root/templates`).
While it holds, the pieces of the logs pane that belong to the selected note are suppressed — the links footer, the scale-chain crumbs (replaced by `templates / <name>`), the captured-today block — and Escape hands the pane back to the selected note.
Selecting any rail or grid note exits the same way, since selection always re-derives the editor.

`template.typ` (the shared defs file) is editable too: excluding it costs filter code for no safety gain; its non-import blocks render near-empty pages, but the active block always shows source and saves are atomic.
Each autosave of a template fires `VaultChange::Template` and clears both render caches (`adr/2026-08-template-touch-clears-caches.md`) — churny once per 500 ms quiet window, accepted as the price of correctness.

## Alternatives rejected

- **A dedicated template editor surface** — a second editor is exactly what the buffer/widget separation exists to avoid; autosave, conflict handling and vim come for free in the one editor.
- **Generalizing the table's sheet to hold paths** — the sheet is keyed by note id through the index, and templates have no index entry; widening that seam for one feature buys nothing over the logs pane.
- **A tracked `template_mode` flag** — a second source of truth beside the buffer's path, and one more thing every exit path must remember to clear.
- **Creating new templates in-app** — a template is only reachable through a `NoteType`, and the type vocabulary is a closed enum in source (`create::TYPES`); a new type is a source-level change, so a new template is too.
