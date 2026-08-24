# The claude CLI runs only when asked

## Context

`plan.md` § AI integration says the app "shells out to the `claude` CLI headlessly" but never says *when*.
The options span a spectrum: a palette command run by hand, a debounced background run after each save, or a scheduled vault sweep (`app --suggest` under a systemd timer).
Every run costs API money, and the system's ethos is that features earn their way in through demonstrated friction ("as needed"), not through completeness.

## Decision

Suggestion generation is a **manual palette command** — "Suggest links · this note" to start — and nothing runs `claude` without a keystroke.

- No surprise cost: every API call traces to a hand on the keyboard.
- No background-process story in phase 2: no debouncing, no run-queue, no staleness protocol between a sweeping daemon and an editing user.
- The trigger sits in the palette like every other command, visible and searchable, per the palette invariant.
- Ambient triggers can still earn their way in later through daily friction — "I keep forgetting to run it" is exactly the signal the polish backlog exists to catch.

## Alternatives rejected

- **Background on save** — ambient and zero-effort, but continuous cost proportional to writing (the thing the system optimizes for doing constantly), plus a debounce-and-cancel story on day one of the engine.
- **Scheduled batch** (`app --suggest` + timer) — decoupled and cheap to reason about, but suggestions lag a day behind the writing they should connect, and the second process needs its own store-locking answer before the store has even stabilized.
