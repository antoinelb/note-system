# The time rail scrolls continuously, newest first

## Context

The logs screen's left pane is one list where indentation carries the scale
(season ⊃ week ⊃ day).
The design deliberately left open whether the rail scrolls through all time
notes or pages by month (`design/wireframes-v0.md` § Open knobs, roadmap
phase 7 **→ ADR**).

## Decision

- The rail is **one continuous list of every time note**, scrolled natively.
- Order is **newest first**: today sits near the top, the past trails down —
  a log is consulted from the present backwards.
- The month grid is the only paging surface ("months page by scrolling"
  already belongs to it); the rail never tracks a month filter.

Consequence for the index: the range-bounded query the roadmap predicted is
not needed.
The rail wants *everything* (`Index::time_notes`), the month grid derives
existence from that same in-memory set, and the "captured today" block is a
`created = ?` equality query (`Index::captured_on`) — simpler than a range,
and a single-user vault stays small for years.

## Rejected

- **Page by month**: tighter visual coupling with the calendar, but it adds
  a month-filter state the rail must track, duplicates the grid's paging
  role, and needs the date-bounded query for no other benefit.
- **Oldest first**: reads like the calendar, but puts today at the bottom
  and needs an auto-scroll on launch.
