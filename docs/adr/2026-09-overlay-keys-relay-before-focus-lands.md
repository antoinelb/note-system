# A key that beats an overlay's focus grab is relayed into the overlay, never read by the grammar

## Context

Every overlay grabs its own focus in an async `onmounted`
(`adr/2026-08-palette-order-and-overlay-placement.md`): the chord opens
it, the render mounts its input, the mount handler asks the webview for
focus. Between the chord and that grab landing, the keyboard still points
at whatever held it before — the `ime-sink` under an active block, or a
pane — and the sink hands every key to the vim grammar unconditionally.

The 2026-09-02 audit's `make e2e` run caught it: `notice-resolves.test.sh`
pressed Ctrl+N and typed "concept", the "c" landed on the note behind as
a pending change operator, the type picker saw "oncept", and the vault
ended up with `permanent/onceptevergreen-notes.typ`. A stray "x" or "dd"
in that window edits the note behind. Every e2e scenario already paced
its keystrokes behind a screenshot round trip to dodge this — the suite
worked around the defect rather than catching it.

## Decision

**One `relay` callback, asked first by the sink's keydown and by both
panes' keydowns.** Given a key and its modifiers it answers whether the
key belonged to an overlay:

- **Escape and every Ctrl/Alt/Meta chord pass through** (`false`): the
  escape ladders and the chord arms own those whether or not an overlay
  is up, and the overlays' own Escape handlers stay the first rung.
- **With a query-bearing overlay open** (palette, creator, link picker,
  filter, jump, template, search, ex line, recent notes) a `Character`
  is pushed onto that overlay's query signal, `Backspace` pops one, any
  other key is dropped, and the answer is `true`.
- **With a list overlay open** (loops, settings, notices) every bare key
  is dropped, `true`.
- **With nothing open**, `false`, and the grammar speaks as before.

The sink stops propagation on a relayed key so the pane never relays it a
second time. The nine query inputs become **controlled** (`value:`), the
creator's already was: an uncontrolled input would show nothing for the
relayed letter, and its next `oninput` would overwrite the signal with
its own shorter value. This does not touch
`adr/2026-08-ctrl-l-link-picker.md`'s rejection of a controlled widget —
that was the editor's textarea and its caret, not a one-line query.

The overlay's own `oninput` and `onkeydown` are unchanged: once focus
lands, the input owns the keys as it always did.

*Amended 2026-09-02:* `oninput` is no longer unchanged. The patch that
carries a relayed letter races the focus grab, so a report from the
field is read as a delta against what it may still be showing
(`adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md`).

## Known ceiling

Enter, arrows and the rest are dropped, not forwarded — forwarding Enter
means nine accept paths lifted out of nine `onkeydown` closures. A user
who presses Ctrl+B then Enter inside the grab's window presses Enter
again. The e2e scenario paces only its Return for this reason. Lift the
accept paths into callbacks if that repeat ever shows up in a session
report.

## Alternatives rejected

- **Swallowing the keys without relaying** — closes the data-mutation
  hole but still loses the letter, and a lost first letter is what the
  audit run showed: "oncept". The relay costs one signal write more.
- **Making the sink the only keyboard source and drawing overlays from
  state** — no race at all, but every overlay's Enter, arrows and Escape
  move into the relay and the overlays stop being self-contained, the
  property `adr/2026-08-palette-order-and-overlay-placement.md` chose.
- **A synchronous focus grab** — the input does not exist until the
  render lands in the webview; there is no earlier moment to ask.
- **Pacing every keystroke in every e2e scenario** — what the suite did;
  it is a test-side workaround, and a person typing a title after Ctrl+N
  has no screenshot to wait for.
