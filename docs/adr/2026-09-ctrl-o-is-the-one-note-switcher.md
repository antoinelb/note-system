# Ctrl+O is the one note switcher

## Context

Two pickers answered "open a note by name", and neither answered it well.

`adr/2026-08-ctrl-b-recent-notes-picker.md` gave Ctrl+B the visit log:
distinct notes, newest first, the place currently showing left out.
It knows only where you have already been, so a note never visited in the
session is unreachable through it, and an empty log made the chord a
silent no-op — the one gesture in the app that answers a keystroke with
nothing at all.

`adr/2026-08-jump-ctrl-o-centres-viewport.md` gave Ctrl+O the whole index
under `links::filter`, but table-only, restricted to ids that have cards,
and it did not open anything: it panned the viewport so the card sat in
the middle.
Reaching a note by name therefore meant Ctrl+O, then finding the centred
card, then clicking it — and on the logs the chord did not exist.

Both chords are Obsidian's; the app they replace binds Ctrl+O to exactly
one thing, a quick switcher that opens a note from anywhere.
The muscle memory the jump ADR claimed for Ctrl+O is the muscle memory of
*opening*, not of panning.

## Decision

**One overlay, on Ctrl+O, on every screen — logs, sheet and table.**
It is a `command-palette` box in the pickers' one grammar: a head reading
"open note", a query input, arrows over a clamped highlight, Enter,
Escape, clickable rows.
Its input goes through the `Shown` delta idiom like every other query
input (`adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md`)
and it joins the relay's roll-call, so a key typed before its focus grab
lands reaches the query and never the grammar
(`adr/2026-09-overlay-keys-relay-before-focus-lands.md`).
The webview owns Ctrl+O as an open dialog, so both arms keep
`prevent_default`.

**Two lists, one row shape, both frozen at open.**

- **With no query, where you have been**: the visit log's distinct notes,
  newest first, the note currently showing left out — what `open_back`
  computed.
  Unlike Ctrl+B, an empty log is *not* a no-op: the switcher opens on an
  empty list so the query can be typed, and says which emptiness it is
  ("no note visited yet", against the query's "no matching note").
  Opened and dismissed with Enter on the first row, the switcher is still
  the back button Ctrl+B was.
- **With a query, everywhere you could go**: every note the index knows,
  matched by id and title and capped the way `links::filter` matches and
  caps for Ctrl+L — time notes and untyped notes included, which the jump
  overlay's card restriction excluded.

Both halves leave out the note currently showing, **by id**: a visit is a
note either way, so the log's two surfaces (`Visit::Logs`,
`Visit::Sheet`) fold into one row per note rather than one per surface.
Rows carry `links::Completion`, so a recent note draws with the index's
own title when the vault still holds a row for its id.

**Landing follows the category rule a loop line follows**
(`adr/2026-09-loop-lines-open-their-notes.md`): a `time/` note lands on
the logs with its day, week or season selected, where its rail, calendar
and crumbs are; everything else opens the sheet the table hosts, which
from the logs means switching screens exactly as a loop line does.
The verdict is read off what the app already holds rather than a second
index read: every row came from `completions`, and `table_notes` is
exactly the notes outside `time/` that have an id, so "no card claims
this id" *is* the leading directory's answer — and a time file whose stem
no scale can parse falls through to the sheet, the one surface that shows
any file, as `open_loop` lets it.

**Landing is a real visit**, as Ctrl+B's was: `select` and `show_sheet`
push it, so the switcher can bounce, and no push is suppressed.
`restoring_history` survives with its one user, the template-editing
Escape.

**Flush discipline is the sheet's**: the buffer being left reaches disk
before the note is replaced, and the switcher closes only once the
landing really happened — a refused flush leaves the picker up, looking at
the list, rather than closed over a note that never moved.

**A broken index costs the typed half only.** The visit log is app state
and needs no read, so the switcher still opens on it with the notice on
the status line saying why the rest is empty.

**What goes.** Ctrl+B, its "recent notes" palette row and `Back`,
`back_query`, `open_back`, `back_to`; the table's jump-to-card, its
`Jump` state, `open_jump`, `jump_to` and `table::centre_on`.
The palette gains one row, "open note", chord `ctrl+o`, available on every
screen; the registry is 33 commands.
The visit log itself — `history`, `push_visit`, `Visit`, `HISTORY_CAP` —
is untouched, and so is `restoring_history`.

Supersedes the gesture half of `adr/2026-08-ctrl-b-recent-notes-picker.md`
(the log, its dedup and its exclusion rule live on, read by this overlay)
and the whole of `adr/2026-08-jump-ctrl-o-centres-viewport.md`.

## Rejected

- **Keeping Ctrl+B beside it** — two chords for one question, and the one
  they would divide is "which notes do I get?".
  The switcher answers both: the log is what an empty query shows, so the
  recency list is not lost, it is the default view.
  A second chord would only save the user from typing nothing.
- **Rebinding Ctrl+O to the recency list alone** — the smaller change, and
  it keeps the defect: a note never visited this session stays unreachable
  by name, which is the whole reason a quick switcher exists.
- **Fuzzy matching** — Obsidian's switcher scores subsequences, and the
  ranking that makes it feel good is a scorer plus a tie-break plus a
  highlight of the matched characters.
  `links::filter`'s case-insensitive substring over id and title is what
  Ctrl+L already teaches, and one matching rule across both places a note
  is named is worth more here than a better one in a single place.
  Revisit if a vault ever grows past the point where a substring returns
  more than the eight rows the cap shows.
- **Keeping the jump as a separate table gesture** — panning to a card
  without opening it is a viewport command, not a navigation one, and
  nothing asked for it in a session report; `zz`-style recentring on the
  canvas can come back on its own chord if it is ever missed.
- **Resolving the row's path from the index at landing** — the honest
  reading of "the loops list's category rule", and it costs a second index
  open plus three new failure paths (open, query, no row) for a verdict
  `table_notes` already holds. The two agree by construction.
