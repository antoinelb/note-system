# A linewise delete reaching the note's end takes the line's own newline, or the preceding one if it has none

## Context

`linewise_delete` had one rule: delete the span, land the caret on the first non-blank of whatever now follows. That is correct for every line but the note's last, because the note's last line before this fix never carried its own trailing newline — it is the line the trailing empty block exists for (`adr/2026-08-cursor-always-in-the-note.md`), and deleting it with only its own span emptied the line in place rather than removing it: `dd` on the last of two lines left a blank line behind instead of the previous line becoming the new last, the one behavior vim itself never has. Plan item 9 asked for `dd` (and by the same operator path, `dj`, `dG`, `dip`/`dap`, and visual-line `d`) to delete the current line properly: the last line moves the caret up to the previous one, and a single-line note empties in place rather than vanishing to nothing.

## Decision

`linewise_delete` gains a second branch: when the span reaches `text.len()` and the byte before the span's start is `\n` (there is a previous line to slide up), the deleted span grows backward by one byte to take that preceding newline instead of the line's own trailing one it doesn't have. The caret lands on the first non-blank of the line that newline used to separate — now the note's last. A single-line note (nothing before the span) falls through to the existing third branch unchanged: the span alone is deleted, leaving an empty line rather than removing the note's only line entirely.

This changes every linewise `Operator::Delete` caller through the one shared function: `dd`, `dj`, `dG`, `dip`/`dap` when they resolve linewise, and visual-line `d`. All of them now match vim's own last-line behavior, not only `dd`; splitting a "last-line special case" into each caller separately was rejected in favor of the one shared function every one of them already funnels through.

One undo step, unchanged: this is still a single `Operator::Delete` resolution, one `Act::Checkpoint` before the one `Act::Splice` — the extra byte just widens the span the same splice already covers, per `adr/2026-08-undo-at-vim-grain.md`.

## Rejected

- **Special-case `dd` alone** — the bug is in the shared linewise-delete path, and `dj`/`dG`/text objects/visual-line `d` all reach a note's last line by the same operator resolution; fixing only the one-key case would leave the others wrong.
- **Insert a synthetic trailing newline on the buffer instead** — would make the note's own text disagree with what is on disk and what the trailing-empty-block invariant (`adr/2026-08-cursor-always-in-the-note.md`) already establishes without one.
