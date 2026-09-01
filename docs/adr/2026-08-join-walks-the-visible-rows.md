# J walks the visible rows, not the raw newlines

## Context

`J` was missing. The obvious implementation — find the next `\n` and replace it
with a space — is wrong in this editor, because not every newline in the buffer
belongs to a line. A block's trailing separator carries its own newline, and a
blank line between two paragraphs is bytes no caret may rest on
(`adr/2026-08-motions-on-visible-lines.md`).

## Decision

- **The substrate is `motions::Lines`, the visible-row table**, exactly as
  every other line-scoped motion's is. A join replaces the bytes *between* two
  rows — whatever they are, one newline or a whole block separator — with a
  single space, or with nothing for `gJ`.
  Joining across a separator therefore collapses two blocks, which
  `Act::Splice` already routes and re-segments
  (`adr/2026-08-editor-splice-cross-block.md`). No special case.
- **`[count]J` joins that many rows**, with vim's off-by-one: `1J` and `2J` both
  join two. The note's last row has nothing below it and the key is consumed.
- **vim's four no-space rules are kept**: no space when the accumulated text is
  empty, when it already ends in whitespace, when the row being pulled up is
  empty, or when it opens with `)`.
  The "already ends in whitespace" case can only arise from a row *inside* a
  multi-line construct — an ordinary line's trailing spaces belong to its block
  separator, not to the row — which is what its test uses.
- **`gJ` takes the next row exactly as it stands**, indent included, where `J`
  trims the leading blanks.
- **One splice, so one `Act::Checkpoint` and one `u`** for a whole `3J`
  (`adr/2026-08-undo-at-vim-grain.md`). The caret rests where the join
  happened — on the space it inserted — clamping back onto a cluster when the
  pulled-up row was empty and there is nothing at the boundary to rest on.
- `Change::Join { spaced, count }` records it, so `.` repeats it.

## Rejected

- **Splicing each boundary separately** — N splices, N checkpoints, and `u`
  would take a `3J` back one line at a time.
- **Scanning the raw text for `\n`** — would join a paragraph to the blank line
  after it while leaving the block map's idea of the separator behind, and
  every case would need a "but not the separator" clause.
- **`joinspaces` (two spaces after a sentence)** — off by default in nvim and
  not set in the user's config.
