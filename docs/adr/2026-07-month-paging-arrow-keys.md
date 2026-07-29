# Calendar header controls: ‹ today ›, plus arrow keys

## Context

The wireframes said the jump panel pages months by scrolling only ("no
chips, no ‹ › buttons", `design/wireframes-v0.md` § The logs screen).
After living with the screen, a 2026-07-28 mockup amended that: the
calendar header gains ‹ › chevrons and a **today** button, right-aligned
beside the month label. The mockup is the newer design word and wins.

## Decision

- The cal header shows `‹ today ›`: the chevrons page the month grid one
  month back / forward (the same `logs::page_month` the wheel uses); the
  **today** button selects today's day note — the centre pane follows and
  the month snaps back — without ever creating a file (only Enter writes).
- ArrowLeft / ArrowRight on the logs pane are the keyboard equivalent of
  the chevrons: they move the grid only, never the selection.

## Rejected

- **Keystrokes only** (the wireframes' original position): the amended
  mockup explicitly draws the header controls.
- **A today button that creates the note**: creation stays behind Enter on
  an empty selection, keeping "navigating never writes" intact.
