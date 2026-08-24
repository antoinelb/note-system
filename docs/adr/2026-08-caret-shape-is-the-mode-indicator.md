# The caret's shape is the mode indicator, and modal keys take no palette entries

## Context

The design's chrome refuses labels; a "-- INSERT --" line has nowhere to live.
The owned caret can finally draw the box WebKitGTK never could — the reason the widget was built first (`adr/2026-08-v2-caret-first-order.md`).

## Decision

- **Bar writing, box thinking**: insert draws phase 0's blinking bar; normal draws the box — the cluster under the caret inverted in the ember, a no-break-space stand-in at a line's end. The flip is one `Shape` parameter into `caret::layout`; nothing else moves. The box does not blink — it is a state, not an invitation.
- **The palette boundary**: modal keys are a grammar, not commands — `i`, `Escape`, and every later motion and operator take **no** palette entries. Chords keep theirs and answer in both modes: `Vim::handle` passes any ctrl/meta keystroke through before the mode even looks at it.

## Rejected

- **A mode word in the chrome** — the one-line chrome gains nothing and the design language loses (the recorded reason this ADR exists).
- **Palette entries for `i`/`Escape`** — a grammar enumerated as commands is noise; the palette lists intents, not letters.
