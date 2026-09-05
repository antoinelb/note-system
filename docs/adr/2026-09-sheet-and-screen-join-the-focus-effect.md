# The sheet and the screen join the pane-holds-focus effect

> Superseded by `adr/2026-09-the-sink-is-the-one-keyboard-socket.md`: the focus effect is gone. The sink holds the focus for the life of the window and the `hosted` reading — the grammar speaks only where the note is drawn — moved into the one key dispatcher.

## Context

A note-taker session found the table screen a keyboard dead end after a
sheet closed — Ctrl+2, Ctrl+D and Ctrl+P went nowhere — and found
keystrokes swallowed after clicking a chrome icon to switch screens
(findings #3, #4, #6 of `docs/plans/2026-09-01-note-taker-findings.md`).
The chords themselves were already wired: `table_keys` handles Ctrl+2,
Ctrl+P and Ctrl+D exactly as the logs arm does. What failed was focus.

`adr/2026-08-the-pane-holds-focus.md` gave the logs pane one shared
`use_effect` that re-requests focus onto `sink` or `pane` whenever a
tracked flag changes. The sheet reuses that one editor
(`adr/2026-08-sheet-reuses-the-one-editor.md`), so when v1 added the table
screen and the sheet, the effect kept working by accident for the common
case — but it read `editor.read().active()` and a list of overlay flags,
never `sheet` or `screen`.

Two gaps followed from that:

- **`blocks_view` — and the `ime-sink` input inside it — only mounts on
  the logs screen or behind an open sheet.** The table screen with no
  sheet renders neither. But closing a sheet can still leave the one
  editor `active`: the daily note wakes with its last block active on
  open (`adr/2026-08-cursor-always-in-the-note.md`). The old effect saw
  `editing == true` and aimed at the sink — a cell nothing had mounted
  since the sheet's own sink just unmounted with it. `set_focus` on that
  stale handle answers no one, and focus is stranded on `<body>`, which
  is *above* `.app` and outside every listener the pane and sink share
  (the same failure shape `adr/2026-08-the-pane-holds-focus.md` first
  diagnosed for Ctrl+Q).
- **A mouse-driven screen switch changes `screen` and nothing the effect
  read.** The only thing asking for focus was the newly mounted pane's
  own `onmounted`, racing WebKitGTK's native click-focus with nothing
  backing it up if that race is lost.

## Decision

**Read `sheet` and `screen` in the same effect, every run, and use them to
decide whether a mounted sink actually exists to receive focus:**

```rust
let sheeted = sheet.read().is_some();
let hosted = screen() == Screen::Logs || sheeted;
let target = if editing && !listing && hosted {
    sink.borrow().clone()
} else {
    pane.borrow().clone()
};
```

- `hosted` names where `blocks_view` actually renders: the logs screen,
  always; the table screen, only behind an open sheet. A block being
  `active` is necessary but no longer sufficient — the sink has to be a
  real mounted thing, not a leftover handle.
- Reading `screen()` also makes every screen switch re-run the effect, not
  just the newly mounted pane's own `onmounted`. The two are not in
  competition: the pane's own mount request stays (an unmounting pane
  takes nothing with it that the next one doesn't reacquire), and the
  effect is the belt to its braces — whichever request lands last simply
  wins, exactly as `adr/2026-08-the-pane-holds-focus.md` already treats
  every focus request as disposable and shruggable.
- A sheet with no active block still ends on the pane: `editing` is false
  there regardless of `hosted`, so the table screen's own chords keep
  reaching it the moment the sheet closes.

No new state: `sheet` and `screen` were already signals the effect's own
component owns: the fix is reading two more of them, not building
anything.

## Rejected

- **A per-screen effect** (one for the logs pane, one for the table pane)
  — the two panes already share one `pane` cell and one `sink` cell
  because they share the one editor; splitting the effect would just
  reintroduce the "the fifth site added later silently reintroduces the
  bug" risk `adr/2026-08-the-pane-holds-focus.md` rejected for
  refocusing-at-each-site in the first place.
- **Relying on `onmounted` alone for the screen-switch half** (item 4) —
  it already asks for focus on every mount, but a request racing the
  webview's own click-focus with nothing behind it is exactly the shape
  of bug this ADR exists to close. Reading `screen` costs one line and
  removes the race's only failure mode: even if the mount request loses,
  the effect's own re-run (driven by the `screen` signal, not by the
  mount event) asks again.
