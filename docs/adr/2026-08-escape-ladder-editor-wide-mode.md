# The Escape ladder, and one editor-wide mode that survives

## Context

v2 phase 1 introduces normal and insert modes (`roadmap-v2.md` § Phase 1).
The modal layer is `src/vim.rs` — the slot `editor.rs` reserved: the sink forwards keys there first; the grammar answers with editor intents (`Act`), a swallow, or a pass back to the phase-0 keymap.

## Decision

- **The ladder**: Escape in insert → normal (the caret stepping back onto the last typed cluster, as vim leaves it); Escape in normal → the block renders again (`deactivate`); with no block active the pane's existing Escape closes the sheet or the loops list — three rungs, one key.
  This deliberately supersedes half of `adr/2026-08-cursor-always-in-the-note.md`: the fully-rendered state returns as a rung. A note still *opens* with its cursor placed; Escape can now put the cursor away.
- **One mode, editor-wide, surviving**: `Signal<Vim>` lives beside the editor signal in `Shell`, captured by both mounts. A boundary slide, a fresh activation, a sheet open — none of them reset the mode. Not on `Editor`: the mode belongs to the layer above it, or the seam the whole version exists for dissolves.
- **Insert at launch**: the roadmap fixes the ladder but not the birth mode. This app opens on today's note *to write*; insert-at-launch keeps the open-and-type flow, and Escape is how the editor starts thinking.
- **Insert entries**: `i a I A o O`, placements computed pure in `vim.rs` over the block's source (`a` never crosses the line end; `I` lands on the first non-blank; `o`/`O` open a line with a `\n` inside the block — a blank line splits at the next resegmentation, the existing semantics).
- **Normal mode residue**: the phase-0 arrows and Home/End still pass through until phase 2's motions replace them; every unbound key — including AltGr characters, which carry alt — is swallowed inert. A composition started in normal mode is discarded whole at commit.

## Rejected

- **Per-activation mode reset** — a slide mid-thought would eject to normal (or insert), and the roadmap's suggestion was survival.
- **Normal at launch** — vim's default, but this editor's front door is a daily note that exists to be written in; the vim reflex `Esc` is one key away.
- **Mode on `Editor`** — the keymap layer is *between* widget and editor by construction (plan.md § Editor); the editor must stay ignorant of modes.
