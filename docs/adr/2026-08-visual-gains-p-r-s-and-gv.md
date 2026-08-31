# Visual mode earns p, r, s and gv — and visual p does not clobber the clipboard

## Context

`adr/2026-08-visual-selection-is-the-anchor.md` shipped visual mode with
motions and the three verbs, and closed with "`p` over a selection,
`viw`-style objects in visual — the roadmap's phase-4 list is motions +
operators; the rest earns its way in." Daily writing asked for the rest.

## Decision

- **`s` is `c`.** In vim the two are identical over a selection. `S` is *not*
  vim's line-wise change here, because surround owns it
  (`adr/2026-08-surround-pair-set-and-padding.md`) — which is exactly what
  `vim-surround` does to the same key in the user's own vim, so the two agree.
- **`u`, `U`, `~`** are the case verbs from
  `adr/2026-08-case-operators-are-verbs.md`, through the existing
  `visual_operate`.
- **`r{char}`** overwrites every cluster of the selection, newlines excepted —
  they keep the lines apart, or a `V`-selection's `r` would merge the note into
  one line. The count is ignored, as vim ignores it here. An empty selection
  splices nothing and checkpoints nothing.
- **`gv` puts the last selection back** as `Act::Place(anchor)` followed by
  `Act::Extend(head)` — **no new act**. That falls out of the phase-4 decision
  that the selection *is* phase 0's anchor: with no separate visual state to
  restore, restoring it is two moves the executor already knows.
  The selection is remembered by **one seam**: `Vim::handle` stamps
  `(kind, anchor, head)` on every keystroke that reaches visual mode, so
  whatever the last such keystroke saw is exactly the selection the mode died
  with — no bookkeeping at each of the half-dozen exits (Escape, an operator,
  a kind toggle, Tab, a pending wrap).

## Visual p does not clobber the clipboard — a deliberate divergence

Vim puts the replaced text into the unnamed register. Here the register **is**
the OS clipboard. Following vim would mean that pasting a phrase over the first
of five spans destroys the phrase, and the remaining four pastes would insert
whatever was just deleted. Pasting one thing over several spans is the main
reason to reach for visual `p` at all, so the vim behaviour would break the key
in its primary use.

`Act::PasteOver { span, linewise }` therefore reads the clipboard, splices, and
writes nothing back. A line-wise selection keeps the clip's own trailing
newline; a char-wise one drops it and pastes inline.

## Rejected

- **Vim's register swap** — see above. This is the second place the one-register
  rule has forced a divergence, and both times the clipboard's persistence
  across applications is what made vim's answer wrong here.
- **Keeping the selection alive after a visual verb** — vim spends it, and `gv`
  now brings it back when that is what was wanted.
- **A `Vim` field per visual exit** — one seam in `handle` beats six call sites
  that must all remember.
