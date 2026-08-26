# Todo fixes: cursor-split rendering, settings page, and chrome polish

## Goal
Every unchecked item in `todo.md` (lines 14–29) is implemented or resolved, with the rendering model moving from one compiled fragment per line to two fragments split at the cursor.

## Out of scope
- No change to the editing unit: the active line stays the textarea, and dd, ip/ap, and motions keep their per-line meaning from `adr/2026-08-per-line-block-segmentation.md`.
- No performance work beyond what the two-fragment model gives for free.
- Todo 17 (`+` lines same block) is subsumed by the rendering change and needs no separate work.

## Constraints
- The user decided (2026-08-25): rendering only for the cursor split; blank lines show real vertical space everywhere, including adjacent to the active line and when the cursor sits on one; the loops palette command works from every screen; Ctrl+Shift+D deletes immediately with no confirmation, superseding `adr/2026-08-delete-note-palette-only-from-sheet.md` (new ADR required).
- Parsing stays on `typst-syntax`, never regex or character counting.
- No colour literals outside `assets/theme.css`; all UI strings English; spacing in multiples of 4 UI pixels.
- Decisions made during implementation get their own ADR under `docs/adr/`.
- Dioxus work reads `.claude/dioxus.md` first.
- 100% coverage after `make test`; clippy warnings are errors.

## Items
1. Research how live-preview editors (emacs org-mode/latex-preview, vscode markdown/latex extensions, TeXpresso-style tools) split rendered and source regions around the cursor, producing a short written comparison that informs item 2 (todo 29).
2. Rendering split at the cursor: everything above the active line compiles as one Typst fragment and everything below as another, blank lines render as real vertical space everywhere including next to the active line, and the per-line fragment cache and its cost ceiling are retired (todos 19, 17; supersedes the rendering half of `adr/2026-08-per-line-block-segmentation.md` with a new ADR).
3. Rendered output and the editor textarea use the same font size (todo 14).
4. Visual mode selects across multiple lines, extending the existing visual-line machinery in `vim.rs`/`motions.rs` beyond a single block (todo 15).
5. A settings page opens on Ctrl+, holding a theme toggle and a font size control (todo 16).
6. Rendered todo items respect their indentation level (todo 18).
7. Every button gets an HTML `title` hover tooltip (todo 20).
8. The notices/status-history pane closes on Escape and on click, per the escape ladder (todo 21).
9. Ctrl+B navigates back through previously opened notes, Obsidian Ctrl+O-style history (todo 22).
10. The palette drops the capture-clipboard command and lists all commands alphabetically, and the open-loops command opens the loops overlay from any screen (todos 23, 24).
11. Ctrl+D on the table opens today's daily note in the temporal screen, and Ctrl+T toggles a todo only when an editor has focus (todos 25, 26).
12. Ctrl+Shift+D deletes the current note immediately without confirmation, with an ADR superseding palette-only delete (todo 27).

## Acceptance
`make static && make test` green with 100% coverage, every behaviour above covered by tests, and the corresponding boxes in `todo.md` checked.

## Check
`make static && make test`
