# Todo fixes: hotkeys, palette navigation, line-level blocks, table bugs

## Goal
Every item in `todo.md` is applied: c-t becomes the todo toggle, c-d opens the daily note, the palette gains daily/weekly/season navigation and loses calendar-widget duplicates, quotes render full width, block segmentation becomes per-line, and the table editor's escape, empty-line, rendering, and dd behaviours are fixed.

## Out of scope
No new palette commands beyond the nine navigation ones.
No change to the vim grammar beyond `dd`.
No change to the calendar widget itself.
Theme toggling stays available through the palette ("toggle theme"); only its hotkey goes away.

## Constraints
Strict buffer/widget separation in the editor (CLAUDE.md); what edits text as you type belongs to the editor, not the grammar.
Segmentation must keep tiling the whole note (`src/blocks.rs` invariant: blocks tile the text) and every fragment must still compile standalone via `template.typ`.
Read `.claude/dioxus.md` before touching any Dioxus code.
`make test` requires 100% region/line/function coverage.
All UI strings in English; no colour literals outside `assets/theme.css`.

## Items
1. Hotkeys — remove the c-t → toggle-theme binding (`src/keymap.rs:242`), bind c-t to a todo toggle in both editors: a plain line gets prefixed into `- [ ] `, a `- ` item becomes `- [ ] `, `- [ ]` and `- [x]` toggle each other, the checkbox is never removed.
2. Hotkeys — bind c-d to open today's daily note from anywhere the palette's "go to today" works today.
3. Palette — rename "go to today" to "open daily" and add "open previous daily", "open next daily", plus the same trio for weekly and season: next is the following period relative to the open note (created from template when missing), previous is the most recent existing note before it.
4. Palette — remove "previous month" and "next month"; the calendar widget keeps those interactions.
5. Rendering — block quotes take the full width of the rendered note instead of their natural width.
6. Blocks — segmentation (`src/blocks.rs`, today parbreak tiling) becomes per-line: each line is its own block, lines with an unclosed `[`, `(` or `$` merge with the following line(s), and the leading run of `#import`/`#show`/`#meta` lines stays one preamble block; the exploration must flag risk cases (raw fences, multi-line `#let`, Typst tables) and merge them when a line cannot compile alone.
7. Table — Escape no longer closes the note modal; only Shift-Escape does.
8. Table — a line the cursor sits on no longer disappears when it is empty, and note lines render even when the cursor is not on them (both symptoms of the modal's per-line rendering).
9. Editors — `dd` deletes the current line in both the main editor and the table modal: deleting the last line moves the cursor to the previous one, and the only line is emptied rather than removed, as one undo step.

## Acceptance
`todo.md` items all resolved and verifiable in the running app.
`make static && make test` green with 100% coverage.

## Check
`make static && make test`
