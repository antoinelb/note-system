# One register, and it is the system clipboard

## Context

v2 phase 3 gives the grammar its verbs (`roadmap-v2.md` § Phase 3). Vim has named registers; the roadmap's ceiling says they wait for demonstrated need.

## Decision

- **The one register is the OS clipboard** — vim's `clipboard=unnamedplus` as the only behaviour. `d`, `c`, `y` (and `x`, `X`, `D`, `C`, `Y`) emit their cut through the phase-0 `ClipboardWrite` seam; `p`/`P` read through the `Clipboard` seam. Copying in the browser and `p`-ing into a note — and the reverse — is the point.
- **Linewise-ness is the trailing-newline heuristic**: vim's register kind cannot ride through the OS, so a clip ending in `\n` pastes linewise (`p` opens below, `P` above), anything else charwise. A linewise yank whose span lacked its newline (a block's last line) gains one on the way out, so `dd` then `p` round-trips linewise.
- **Paste resolves pure then splices**: the executor reads the clipboard async, then `Editor::paste` → `motions::paste_spec` decides the insertion span, body and caret against the state the read found. A paste is always an insertion inside the active block. Below a block's last visible line — whose newline belongs to the separator — `p` opens the line with the break the body carried, leaving the separator untouched.
- **`r` fills no register** (vim's own rule); an empty charwise cut writes nothing, so a no-op `D` cannot clobber the clipboard; `dd` on an empty line still yanks `"\n"`, as vim does.
- Objects: `iw aw`, the quote flavours `" ' \``, the pairs `() [] {} <>` **and `« »`** — the notes are French — plus the house `ip`/`ap`: the paragraph *is* the block, read off the block map, `ap` riding the separator.
- House quirks adopted with tests: `cw` acts as `ce` on a word; `dw` on a line's last word stops at the line's end.

## Rejected

- **Named registers** — the ceiling; a second register earns its way in through daily friction.
- **An internal register beside the clipboard** — two places text goes when you cut is exactly the ambiguity `unnamedplus` users configure away.
