# Motions walk the visible lines, and the normal caret rests on clusters

## Context

v2 phase 2 makes the note navigable at the speed of intent: h j k l, w b e, 0 ^ $, f F t T with ; and ,, gg G — all with counts, note-scoped, char-wise over French text.

## Decision

- **The visible-line table is the only substrate line-scoped motions see** (`motions::Lines`): every block content's lines as note-global byte ranges. Block separators are bytes no caret may rest on, so `j` crossing a block boundary is just the next table row — the flush-and-resegment slide falls out of `Editor::place_at`, never a special case. The trailing empty line of the note's last block is a real line (`adr/2026-08-cursor-always-in-the-note.md`).
- **Word motions scan the raw text**: separators are whitespace, and vim's w/b/e skip whitespace by definition, so they cross blocks naturally with no table. Word characters are `is_alphanumeric` or `_` — é œ are word characters, `l'idée` splits at the apostrophe. Vim's empty-line word-stop is not implemented (simplification, revisit on friction).
- **The normal-mode caret rests on clusters**: a line's deepest position is its final cluster's *start* — `l` and `$` land on the last character, not after it, exactly as vim draws its block cursor. Phase 3's inclusive/exclusive operator spans depend on this convention. Insert entries step off it (`a` appends past the cluster, `A` to the line's true end).
- **The goal column is cluster-counted and lives in `Vim`**, remembered by j/k runs, forgotten by everything else. Counts multiply into motions (`3j`, `12l`, `2fc`, `[count]gg` to line N, clamped); a count with no motion dies on Escape — the ladder's pending rung — or resets when a phase-0 arrow passes through.
- **`Editor::place_at`** is every motion's landing: note-global, waking the owning block via the existing activate path when the caret leaves the active one. Phase 5's search lands through the same door.
- **The span-kind table for phase 3**, recorded now so no motion is revisited: `w b h l 0 ^` exclusive; `$ e f t F T ; ,` inclusive; `j k gg G` linewise. (`$` was first written down here as exclusive; the code has always made it inclusive, which is both vim's behaviour and what lets `d$` reach the line's end. The table is corrected, not the code.)

## Rejected

- **Motions over the raw text with separator-skipping fix-ups** — every line motion would carry a "but not into the separator" clause; the table makes the illegal states unrepresentable.
- **A per-block caret space** — `gg`, `dG` and search live at note scope; the editor's caret is already note-global bytes (`adr/2026-08-caret-on-editor-note-bytes.md`).
- **Vim's `curswant=MAXCOL` stickiness for `$`** — j after $ keeps the goal column of the landing, not end-sticky; a simplification noted for the friction backlog.
