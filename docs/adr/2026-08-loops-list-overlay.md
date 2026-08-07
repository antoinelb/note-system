# The open-loops list is an overlay in the centre pane

## Context

`adr/2026-07-debt-counter-then-list.md` decided the ember opens a flat list,
and phase 10 has to draw it. The deck never did: "show the loops screen in
this language" is still in its own *try next*, so there is no wireframe to
transcribe and the surface is designed here, in the phase-6 vocabulary.

The v0 app also has no router — `Screen` is a two-variant enum that only
decides which chrome icon is lit, and the table does not exist until v1.

## Decision

- **Clicking the ember toggles an overlay inside the logs centre pane**,
  built like the Ctrl+L link picker (`.loops-list`, a bordered box with a
  `type-label` head reading "open loops" over plain lines). A second click
  or Escape puts it away.
- **Not a third `Screen`.** The chrome's icons still navigate nowhere, the
  rail and the jump panel stay where they are, and the list is a thing you
  glance at and dismiss — the ADR's "a destination, not a workspace" reads
  as an overlay, not as a screen the app can be stuck on.
- **The count is the list's length.** `survey` returns the lines themselves,
  not a number; the ember renders `lines.len()` and the overlay renders the
  lines. There is no separate counting query that could disagree with what
  the list shows, which is also what makes "the counter total matches the
  list" true by construction rather than by test.
- **One line per loop, in query order** — typeless notes, then dangling
  links, then captures still owing a summary. A note is named by its stem,
  which is its id; the tag after the `·` says which loop it is:
  `mystere · typeless`, `2026-07-22 → fantome · dangling`,
  `capture-zettel · still open`. No ages, no grouping, no per-item actions,
  nothing clickable (`adr/2026-07-debt-counter-then-list.md`).
- **Zero loops still renders nothing at all** — no ember, so nothing to
  click and no empty list to open. Absence, not a zero, unchanged from
  phase 6.
- The list keeps the muted-ink line idiom of the "captured today" block
  rather than the ember's warm hue: the *count* is the alert, the contents
  are just reading.

## Rejected

- **A third screen with its own chrome icon** — the design's chrome is two
  icons and an ember, and adding a screen would need answers about what the
  rail, the crumbs and the create keystroke mean while it is up. All of that
  for a list of at most a handful of lines.
- **A panel replacing the jump panel** — cheaper to place, but the jump
  panel is the time axis; debt is not a date.
- **Rendering the count from its own query** and the list from another —
  two round-trips that can disagree, which is exactly the bug "the counter
  total matches the list" is meant to rule out.
- **Making each line open its note** — the tempting next step, and the one
  the counter-then-list ADR already refused: a typeless permanent note has
  nowhere to open until v1's table, so two thirds of the list would be inert
  and the grammar would be inconsistent.
