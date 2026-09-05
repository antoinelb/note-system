# Time notes sort last in the link picker

## Context

The link picker (Ctrl+L, or typing `[[`) listed `Index::completions()` in the
index's own order — `ORDER BY id` — capped at `MAX_MATCHES` (8).
Every time note's id starts with a year, and digits sort before letters, so
the day, week and season notes take the top of that list and, in a vault of
any age, the whole of it: opening the picker on an empty query in the fixture
vault offers `2026-07-21`, `2026-07-22`, `2026-07-23`, `2026-summer`,
`2026-w30` and only then three permanent notes.

That is backwards for what the picker is for. A day is reached by Ctrl+D, a
week and a season by the rail or a relative-time command, and the id is a
date the user can type from memory in full. A permanent note is the one
whose id the user cannot recall — which is the reason the picker exists.

## Decision

- **The link picker orders permanent notes before time notes**, for the empty
  query and for a typed one alike, and leaves the index's order (by id)
  untouched inside each group. `links::picker_rows` is the new entry point;
  the sort is stable and keyed on a bool, so "inside each group" costs
  nothing to state.
- **The cap falls after the sort.** Taking eight in index order and then
  reordering them would still let a time note occupy a row a permanent note
  wanted; `picker_rows` collects every match, sorts, then truncates.
- **A time note is one whose id parses as a scale** — `logs::scale_of_id`,
  the day/week/season parse the loops list already trusts to route a row to
  the logs. The picker's `Completion` carries an id and a title and no type,
  and the id's own shape is the authority the rest of the app already uses
  for exactly this question; nothing new is read from the index.
- **The Ctrl+O switcher keeps the order it had.** It shares the matching
  rule, not the ordering: `links::filter` still answers what it always did,
  and both now read one private `matching` iterator so the two lists can
  never disagree about what a query matches. Ctrl+O is a *navigation*
  gesture — "take me back to the day I was in" is a first-class use of it,
  and demoting today's note there would cost what it buys in the picker.
  The full-text finder is a separate overlay over the FTS table and never
  touched `filter`.

## Alternatives rejected

- **Sorting inside `filter`** — one line instead of a second function, but it
  reorders the Ctrl+O switcher too, which is the case the ordering is wrong
  for.
- **Excluding time notes from the picker entirely** — `[[2026-07-23]]` is a
  link the vault really writes (the day notes chain to each other), and a
  list that silently omits a linkable note is worse than one that ranks it
  low.
- **Ranking by recency or by link count** — a better answer to "which note
  did you mean", but it needs data the picker does not hold at open, and the
  complaint being answered is coarse: time notes first, permanent notes
  nowhere.
- **Sorting the completions in the SQL** — `completions()` feeds both
  overlays, so the ordering would leak into Ctrl+O by the same route, and the
  id-shape parse does not exist in SQLite.
