# The Escape ladder, and one editor-wide mode that survives

## Context

v2 phase 1 introduces normal and insert modes.
The modal layer is `src/vim.rs` — the slot `editor.rs` reserved: the sink forwards keys there first; the grammar answers with editor intents (`Act`), a swallow, or a pass back to the phase-0 keymap.

## Decision

- **The ladder**: Escape in insert → normal (the caret stepping back onto the last typed cluster, as vim leaves it); Escape in normal → the block renders again (`deactivate`); with no block active the pane's existing Escape closes the sheet or the loops list — three rungs, one key.
  This deliberately supersedes half of `adr/2026-08-cursor-always-in-the-note.md`: the fully-rendered state returns as a rung. A note still *opens* with its cursor placed; Escape can now put the cursor away.
- **One mode, editor-wide, surviving within a note**: `Signal<Vim>` lives beside the editor signal in `Shell`, captured by both mounts. A boundary slide or a fresh activation never resets the mode. Not on `Editor`: the mode belongs to the layer above it, or the seam the whole version exists for dissolves.
- **Normal at open** (revised 2026-08-09, with the user): a new file starts thinking, as vim's buffers do — the launch default is normal, and every editor replacement (rail selection, sheet open or close, delete's landing, Enter-create) calls `Vim::note_opened`, which returns to normal and clears the pending grammar while the dot, the committed search pattern and the last find survive across notes, like vim's registers. The first cut of phase 1 launched in insert ("the app opens to write"); daily reality preferred the vim reflex.
- **Insert entries**: `i a I A o O`, placements computed pure in `vim.rs` over the block's source (`a` never crosses the line end; `I` lands on the first non-blank; `o`/`O` open a line with a `\n` inside the block — a blank line splits at the next resegmentation, the existing semantics).
- **Normal mode residue**: the phase-0 arrows and Home/End still pass through until phase 2's motions replace them; every unbound key — including AltGr characters, which carry alt — is swallowed inert. A composition started in normal mode is discarded whole at commit.

## Rejected

- **Per-activation mode reset** — a slide mid-thought would eject to normal (or insert), and the roadmap's suggestion was survival.
- **Insert at launch** — the first cut's choice ("the app opens to write"); reversed above: the vim reflex wants `i` to be the one key away, not `Esc`.
- **Mode on `Editor`** — the keymap layer is *between* widget and editor by construction; the editor must stay ignorant of modes.
