# The sink is the window's one keyboard socket

Supersedes `adr/2026-09-overlay-keys-relay-before-focus-lands.md`,
`adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md`,
`adr/2026-09-the-sink-outlives-the-active-block.md` and
`adr/2026-09-sheet-and-screen-join-the-focus-effect.md`; amends the focus
clause of `adr/2026-08-palette-order-and-overlay-placement.md` and
`adr/2026-09-the-picker-rides-the-pane.md`.

## Context

`make upgrade` kept failing on the user's machine while the same suite
passed here. The kept vaults and screenshots of the failed runs, and a
probed binary run under the same load, showed four faces of one defect:

- the settings scenario's first letter after `o` lost, the note reading
  `ettings proving ground`;
- the switcher's query reading `plainfilesi` and `pli-ilesi` for a typed
  `plain-files`, so its Return found no row;
- the loops scenario's `i` and its whole sentence lost after a backdrop
  click closed the list;
- the palette's paced Return dropped, so "open loops" never ran.

Every one is a key that arrived while the element meant to read it did
not have the focus. Four ADRs had each patched one window: the relay for
letters typed before an overlay's grab, the delta reader for the patch
that raced the grab, the hoisted sink for the wake that remounted it, the
focus effect for the sheet and the screen. The windows kept opening
because every overlay, both panes and the sink each asked for the focus
in an async `onmounted`, and between the render that removed the old
holder and the round trip that focused the new one the keyboard pointed
at `<body>`, where nothing reads. A controlled `<input>` added a race of
its own: the field reported values against patches it had not yet
received, and no delta rule can tell a Backspace from a stale field —
the interleaving that lost the hyphen is one the memory forgets by
design.

Pacing the scenarios behind a screenshot round trip only made the
windows narrower. Under two suites at once they were wide enough again.

## Decision

**One element holds the focus for the life of the window: the IME sink,
mounted once with the shell, and every key the window receives is read
from its keydown by one dispatcher.**

- The sink lives at the shell's root, beside the chrome, on every screen.
  It has `autofocus`, asks for the focus once in its own `onmounted`, and
  an injected listener (`launch::KEEP_FOCUS`) refocuses it in the same
  event turn whenever a click on something unfocusable moves the focus
  away — a microtask, not a round trip, so no key can land in between.
- `sink_keys` is the one reader, in the order the sink and the panes used
  to compose by bubbling: a composing keystroke is the IME's; an open
  overlay's keys are its own (`overlay_keys`); the Alt folds; the grammar
  over an awake block on a screen that hosts it; and what none took goes
  to the screen's rungs (`logs_keys`, `table_keys`), which became
  callbacks and lost their `tabindex`, their grabs and their own listeners.
- **Overlays never take the focus.** Each query line is a `div` drawn
  from its signal with a placeholder span and a CSS bar; the list
  overlays lost their `tabindex`. Their former `onkeydown` bodies are
  per-overlay callbacks (`picker_keys`, `palette_keys`, …) that recompute
  their rows from state the way their views do, and `overlay_keys`
  routes a letter or Backspace to the open query, Escape, Enter and the
  arrows to the callback, and drops the rest — so the grammar never reads
  `c` as an operator behind an open picker. Enter is no longer dropped:
  the relay's known ceiling is closed.
- The `Shown` memory, the `typed` reader, every `oninput`, every
  `set_focus` but the sink's own, the focus effect and the panes'
  forwarding arms are deleted.

The e2e harness's pacing helpers stay, harmless; nothing needs them any
more, and the unpaced scenarios are the proof.

## Consequences

- A key is read against the state the previous key produced, whatever
  the DOM is doing: opening a note, landing a switch and creating a day
  are synchronous in the editor, so `o` after Enter always finds the
  note open.
- The tests press every overlay key at the sink, the same path the
  window uses; the openers return the sink for both roles they used to
  fill with the input.
- IME composition in an overlay's query is a named ceiling: the sink's
  composition handlers still feed the editor, and a committed CJK string
  reaches no query. French dead keys still compose into a note.

## Alternatives rejected

- **Keeping the controlled inputs and widening the delta memory** — the
  report is ambiguous by construction: `pla` reported can be a Backspace
  from `plai` or an `a` typed on a field that never received `plai`. No
  memory resolves it.
- **A document-level net that redirects stray keys to whatever should
  have the focus** — closes the `<body>` window but leaves the inputs'
  race, and picks its target by guessing the app's state from the DOM.
- **Making the focus grabs synchronous** — there is no earlier moment
  than the render that mounts the element, and the render itself is the
  round trip.
- **Pacing every keystroke in every scenario** — what the suite did; the
  windows scale with load, and a person typing has no screenshot to wait
  for.
