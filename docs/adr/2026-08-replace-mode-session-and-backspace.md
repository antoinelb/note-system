# R replace mode: a session Backspace can undo, and one clipboard write at Escape

## Context

The vim friction batch adds `R`, vim's replace mode, alongside `s`/`S`.
Backspace inside a real vim replace session restores whatever character it overwrote — a behaviour phase 0's grammar had to encode as its own state, not a single splice.
The one register is the system clipboard (`adr/2026-08-one-register-the-clipboard.md`), and every mutating key checkpoints once per change intent (`adr/2026-08-undo-at-vim-grain.md`).

## Decision

- `R` opens a session: one `Act::Checkpoint`, the mode becomes `Replace`, and the box caret draws exactly as it does in normal mode.
  This is a deliberate, accepted trade against `adr/2026-08-caret-shape-is-the-mode-indicator.md`: the caret shape is the design's sole mode indicator, real vim draws replace mode with its own caret, and a live `R` session is visually indistinguishable from normal mode here.
  The box is what `adr/2026-08-caret-shape-is-the-mode-indicator.md` mandates for normal mode ("normal draws the box").
  Reusing it rather than inventing an `R` shape or a chrome badge is the accepted cost, so a reader hitting this later should know the collision was seen, not missed.
- Each typed cluster overwrites the cluster under the caret, recording the original it displaced.
- Past the line's end there is nothing to overwrite, so a keystroke appends instead — the same splice, an empty recorded original.
- Backspace restores the session's most recently displaced original, splicing it back in.
- An original that was empty — a keystroke that had appended — is undone the same way: restoring it simply deletes what was typed.
- Once the session has nothing left to restore, Backspace only moves the caret left, clamped to the line's start — it never crosses into the block's previous line.
- Every other key is swallowed: the arrows must not walk the caret out from under the session's bookkeeping.
- Escape closes the session: the whole run of displaced originals reaches the one register in a single `Act::SetClipboard`, and the caret steps back one cluster exactly as insert mode's Escape does.
- `replace_from` and the recorded `Change::Overwrite` are set together by `R` and read together by Escape, so Escape fills the record unconditionally rather than matching on what it already held — the two fields cannot drift apart within one session.
- The dot replays the whole recorded session as one splice, never a keystroke-by-keystroke walk, and stays in normal mode — the dot never reopens a session.
- `R` takes no count, and a count ahead of the dot has no effect on an `R` replay either — the recorded text alone determines the splice.

## Rejected

- **A checkpoint per keystroke** — vim's replace session is one undo step, matching the change-intent grain of `adr/2026-08-undo-at-vim-grain.md`; checkpointing every cluster would make `u` unusable on a long replace run.
- **A clipboard write per keystroke** — the one register would end up holding only the last overwritten cluster, losing the rest of the session; one write at Escape keeps the whole run available to `p`.
- **Replaying the dot keystroke by keystroke** — re-running the grammar risks Backspace meeting a different original than the one the first session recorded; a single splice of the recorded string is exact and idempotent.
- **A count on `R`** — out of scope for this batch; no daily-writing friction has asked for it yet.
