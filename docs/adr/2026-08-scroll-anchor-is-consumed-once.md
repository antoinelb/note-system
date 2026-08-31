# The scroll anchor is consumed by the mount that uses it, and j/k always recentre

## Context

The caret's `onmounted` hard-coded `ScrollLogicalPosition::Nearest`: a caret
already in view scrolled nothing, and there was no way to say "put this line in
the middle".

The user's own nvim maps `j` to `gjzz` and `k` to `gkzz`, with `scrolloff = 10`.
They keep the caret pinned to the pane's centre on every vertical move.

## Decision

- **`zz`, `zt`, `zb`** arm through a `Prefix::Scroll` and emit
  `Act::Scroll(Anchor::{Center, Top, Bottom})`. Nothing else on `z` binds:
  this editor has no folds, so `za` and `zR` stay inert rather than pretending.
- **`j` and `k` always recentre.** `Act::WalkVisual` sets the anchor to
  `Center` before spawning the walk — the `gjzz` the user already lives in.
- **The anchor is a signal carrying a nonce**, and the caret span's key carries
  the nonce too (`caret-{head}-{nonce}`). Without it a bare `zz` — which moves
  the caret not at all — would not remount the span and would scroll nothing.

## The rule that keeps it honest

**The anchor is consumed by the mount that uses it and falls back to
`Nearest`.** It must never latch.

The caret span also remounts when an async fragment compile lands
(`adr/2026-08-region-recompile-keeps-the-stale-svg.md`) — a re-render nobody
asked for. A latched `Center` would scroll the note out from under a reader who
pressed nothing at all: the interface moving because of a refresh rather than
because the user moved it, which is the failure AIR LAY-2 / Core rule 5 names.
`settle_caret` therefore resets the anchor before scrolling, so the *next*
mount is `Nearest` again unless another keystroke asked otherwise.

## Rejected

- **A scrolloff-style margin** (the user's `scrolloff = 10`) — `scroll-margin`
  exists in CSS but WebKitGTK's `scrollIntoView` does not honour it, and
  reimplementing the arithmetic means measuring the pane on every keystroke.
  Centring on every vertical move is what the user's config actually produces
  anyway; the margin only differs while the caret is mid-pane, which it never
  is under `gjzz`.
- **Making the centring a setting** — configuration is off the v2 list
  (`adr/2026-08-v2-caret-first-order.md`), and the user's answer to the
  question was unambiguous.
- **Latching the anchor until the next keystroke** — one fewer signal write and
  a viewport that jumps on its own; see above.
