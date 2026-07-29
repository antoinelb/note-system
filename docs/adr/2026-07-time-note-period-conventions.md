# Time-note period conventions: Monday's season, created = period start

## Context

Two conventions were left open for phase 7:
`adr/2026-07-seasons-school-semesters.md` deferred "a week belongs to the
season of its Monday", and nothing stated what `created` means for a weekly
or seasonal note (the fixtures used the period's first day without any code
enforcing it).

## Decision

- **A week belongs to the season of its Monday.** The rail groups weeks
  under seasons by that rule, and days stay inside their week's block.
  Accepted consequence: a day can sit under a season that is not its own —
  2026-05-01 is a Friday whose own season id is summer, but it renders
  under winter's w18 block (Monday 2026-04-27); likewise 2027-01-01 sits
  under 2026-autumn's w53.
- **`created` = the period's first day**: the day itself for a daily note,
  the Monday for a weekly, the season's first day (Jan 1 / May 1 / Sep 1)
  for a seasonal — matching the existing fixtures. The app fills it when
  Enter creates a missing time note from its template.

## Rejected

- **A week belongs to the season holding most of its days** (Thursday
  rule): marginally "fairer" at boundaries, but a second rule to explain,
  and ISO's own year rule already anchors weeks by a single pivot day.
- **`created` = the day the file was written**: records a keystroke instead
  of the period; prev/next and range logic would then disagree with the id.
