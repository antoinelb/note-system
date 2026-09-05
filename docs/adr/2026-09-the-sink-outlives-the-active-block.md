# The IME sink outlives the active block

> Superseded by `adr/2026-09-the-sink-is-the-one-keyboard-socket.md`: the sink now lives at the shell's root, on every screen, and the panes forward nothing — they have no keydown of their own. The observation that motivated this decision stands; its fix was one step short.

## Context

The invisible `input.ime-sink` (`adr/2026-08-hidden-ime-sink.md`) was
rendered inside the `.block-active` div, which is keyed on the block's
start offset. Every wake of another block — `o` opening a line, `gg`,
`j` — unmounted that div and the sink with it, and the fresh sink asked
for focus in an async `onmounted`. Between the unmount and that grab
landing the keyboard pointed at `<body>`, above every listener the app
has, and a key typed there was dropped.

`settings-overlay.test.sh` and `open-note-switcher.test.sh` showed it on
2026-09-04 under a loaded machine, one run in four alone: `o` paced, then
"settings proving ground" typed, and the day held "ettings proving
ground". The pacing helper waits one compositor frame after `o`; the
sink's `set_focus` is an `evaluate_script` round trip that can land
after it. An A/B against the build before that day's merges failed the
same way, more often: a pre-existing race, the same family as the overlay
relay (`adr/2026-09-overlay-keys-relay-before-focus-lands.md`).

## Decision

**The sink is a sibling of the blocks, not a child of the active one.**
It renders once inside `.note-blocks`, after the pane loop, and stays
mounted for as long as the note does. A block waking no longer remounts
it, so the focus it holds is never lost across `o`, `i`, a motion or an
undo. The focus effect and the overlay relay are unchanged: they still
aim at the one sink cell, which is now written once per note instead of
once per wake.

The sink is `position: fixed` at the viewport's corner instead of
absolute at the active block's foot. `node.focus()` scrolls its target
into view, and a sink pinned at the note's end would drag the pane there
on every refocus (closing an overlay, leaving the settings). Fixed
elements never scroll; the caret's own mount is the only thing that
scrolls the pane (`adr/2026-08-scroll-anchor-is-consumed-once.md`).

The hidden-sink ADR's "it remounts clean with the block" is withdrawn.
The sink's value was never read and printable keys are prevented, so it
was already always empty; the remount bought nothing and cost the race.

**A bare key that reaches a pane over an awake block is read as the sink
would read it.** The hoist removes the remount, but the sink still takes
focus asynchronously the first time a note mounts (Ctrl+D creating the
day, Ctrl+O landing) and every time an overlay closes. A key typed in
that window reaches the logs pane or the table pane instead, whose own
arms know chords and Escape and dropped everything else. The sink's
keydown body is now one callback, `sink_keys`, and both panes' last arm
hands it a bare key — no Ctrl, Alt or Meta — while a block is awake (the
table pane only over an open sheet, since the bare table hosts no sink).
The sink stops propagation on a composing key it declines, so the pane
never reads what the sink chose not to. Escape and Enter keep the panes'
own rungs: the same ceiling as the overlay relay.

## Rejected

- **Relaying alone, without the hoist.** The overlay relay exists because
  an overlay's input genuinely has to be a new element. The sink does
  not: removing the reason the focus is lost on every wake is smaller
  than catching every key that falls through, and the pane arm is then
  only for the grabs that remain — a note's first mount, an overlay's
  close.
- **A stable `key` on the active block.** The active div still changes
  position among its siblings, and moving a focused node with
  `insertBefore` blurs it exactly like removing it.
- **Absolute at the container's foot.** A refocus would scroll to the
  note's end; see above.

## Ceiling

A CJK candidate window positions itself at the focused input, which now
sits at the viewport's corner rather than under the caret. Dead-key
composition, the case the sink exists for, has no window. If a real IME
matters one day the sink moves back under the caret by inline style, not
by remounting.
