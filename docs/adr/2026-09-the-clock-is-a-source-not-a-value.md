# The injected clock is a source, not a date

Amends `adr/2026-07-today-injected-root-context.md`, which stands in every
other respect: one clock edge, one injected context, nothing below it
reading the wall clock on its own.

## Context

`main` read `time::today()` once and injected the answer as
`ui::Today(Date)`.
A date is a value, and a value does not move: an app opened yesterday and
left running overnight still believed it was yesterday.
Ctrl+D and the palette's "open daily" opened yesterday's note, the
relative-time commands stepped from yesterday, the open-loops list judged
"due within seven days" against yesterday, and the season's `lit` glyph
could sit a semester behind.
The window stays open for days at a time, so this is the ordinary case,
not the edge one.

## Decision

**`Today` carries a clock, and every reader asks it for the date at its own
moment.**

- `time::Clock` is the source: `Live` reads the wall clock at every call,
  `Pinned(Date)` never moves.
- `time::clock()` replaces `time::today()` at `main`'s one edge — pinned
  when `NOTE_TODAY` names a date the parser accepts, live otherwise, which
  is exactly the rule `today_from` already applied to the date.
  `adr/2026-09-note-today-pins-the-clock-for-e2e.md` is untouched: the
  harness pins the same way through the same variable.
- `ui::Today(pub time::Clock)` answers `now()`. Everything that ran on an
  event now calls it when it runs: Ctrl+D, "open daily", the weekly and
  seasonal openers, the relative-time steps, every `compute::touched` /
  `removed` / `rescan` the write seams submit — the capture's among them —
  a new note's `created`, the loops' and due list's survey, and the table's
  card ages.
  The reads that happen while drawing — the table's cards, the season row's
  `lit` — are live by construction, since they run every render.
- The two `use_signal` initialisers that seed the logs (the day selected at
  launch, the month the grid opens on) still read at mount. They are the
  user's selection afterwards, and moving a selection under the user is a
  worse bug than a stale one; Ctrl+D and the header's ‹ today › both put
  it back on the real day.
- Nothing below `main` reads the wall clock except through this context.
  The rule did not change shape, only tense: one clock **source** at one
  edge instead of one clock **read**.

## Proving it

A day change cannot be observed by e2e without faking the clock, and
`adr/2026-09-note-today-pins-the-clock-for-e2e.md` already rejected
`faketime`. So the proof is a UI test: `Clock::Ticking`, a `cfg(test)`
variant holding a `&'static Cell<Date>`, lets
`ctrl_d_follows_the_clock_across_midnight` press Ctrl+D, move the cell one
day, press Ctrl+D again, and see the second press land on the new day.
The variant does not exist in the shipped binary.
The e2e suite keeps proving the pin, unchanged.

## Alternatives rejected

- **Re-reading on window focus** — the fix would depend on a focus event
  the desktop may never send (a window left focused across midnight gets
  none), and it would add a platform seam for a value a plain function call
  already answers.
- **Only Ctrl+D re-reading** — the loudest reader, but the due list, the
  relative-time steps and the capture stamp are wrong in exactly the same
  way; one source read at each reader's moment is fewer rules than a list
  of which readers are allowed to be right.
- **A timer that re-injects the date at midnight** — a task to spawn, a
  wake-up to schedule and a re-render nobody asked for, to avoid a clock
  read that costs nothing.
