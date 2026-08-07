# The logs pane takes focus back whenever no block holds it

## Context

Ctrl+Q (and Ctrl+T with it) stopped working whenever the caret was not in a
block — reported from daily-driving, and reproducible by pressing Escape to
leave a block and then trying to quit.

The cause is the shape of the DOM, not the handlers. Keydown events bubble
*upward*, and the window's chords are handled on the `.app` root
(`adr/2026-07-theme-keystroke-toggle.md`,
`adr/2026-07-ctrl-q-flushes-then-closes.md`), so they only fire for keys
pressed while focus is on `.app` or something inside it. When the active
block's textarea unmounts — Escape, a rail click, following a link — the
webview drops focus onto `<body>`, which is *above* `.app` and therefore
outside every listener the app has. The chords are not broken; they are
never delivered.

The headless tests could not have caught it: `handle_event` dispatches
straight at an element, so bubbling always works there and focus does not
exist.

## Decision

- **The logs pane keeps a handle on itself** (`onmounted` stores the
  `MountedData` in a plain cell, like `QuitFlush`), and an effect asks for
  focus whenever the pane should have it: no active block, or the open-loops
  list is up.
- **Two conditions, both read every time** so the effect follows both. A
  block being edited keeps focus — its own textarea asks for it on mount —
  and the pane does not fight it. The overlay is included because Escape
  closes it and that keystroke lands on the pane.
- **A refused focus request is shrugged off**, exactly as the textarea's own
  request is: headless there is no one to tell, and the caret simply stays
  where it was.
- This also fixes the launch case, where `autofocus` on the pane is a hint
  the webview may or may not honour.

## Rejected

- **A document-level keydown listener installed by JS**, forwarding to Rust
  through `eval`. It is the textbook fix for window-wide chords and would
  survive any focus state, but it means a second event path alongside the
  rsx handlers, hand-written key parsing on the JS side, and a channel to
  keep alive — a lot of machinery for a problem that one focus call solves.
  Worth revisiting if chords ever need to work while a *modal* owns focus.
- **Refocusing at each deactivation site** (Escape, `select`, the link
  chords) — the same call written four times, and the fifth site added later
  would silently reintroduce the bug.
- **Giving up the app-root handlers and moving the chords to the pane** —
  the pane does not exist on the vault-error screen, which is exactly where
  Ctrl+Q needs to work most.
