# A capture's id is the clock, to the second

## Context

Every other note's id is the kebab-case of its title at creation
(`adr/2026-07-id-scheme-kebab-frozen.md`), and time notes use their period.
A capture has neither: "no required fields, no summary required at creation"
(`plan.md` § Capture notes) means there is no title to derive anything from
at the moment the hotkey fires.

The fixture captures (`capture-articles-zettel`, `capture-idea-canvas`) are
hand-written and gave no rule. Ids must also be unique *by construction*,
since a collision is an error rather than a suffixed retry
(`adr/2026-07-id-collision-is-an-error.md`).

## Decision

- **`capture-YYYY-MM-DD-HHMMSS`**, e.g. `capture-2026-08-06-143012`, from
  one clock read; `created` is that same read's date, so a capture cannot be
  filed under a day its id disagrees with.
- **Unique by construction** for a key a human presses: two captures in the
  same second are the collision error, reported like any other. This is
  accepted rather than worked around — a suffix would be the numeric-suffix
  scheme that ADR already deleted.
- **The clock is injected, in both paths.** The headless `--capture` process
  takes `jiff::Zoned::now()` in `main` and passes it to `capture::run`, so
  the logic is deterministic under test. In the app, a `Now` context is
  injected beside `Today` and read only when a capture is written: `Today`
  stays the app's single launch-time clock read that every screen is drawn
  from (`adr/2026-07-today-injected-root-context.md`), while a capture needs
  the time of day and needs it at the moment it arrives.
- **The id is the filename and the `#meta(id:)`**, exactly as for every
  other note. It is also the label shown in the "captured today" block and
  the open-loops list, so a capture is identified by *when* it arrived —
  which, for something with no title, is the only true thing about it.

## Rejected

- **Kebab of the first words of the paste** (`capture-zettelkasten-method`)
  — readable, but two similar pastes collide, an empty paste has no id at
  all, and it would put the *content* into the filename of the one note kind
  whose content is explicitly not yet the user's own words.
- **A date plus a counter** (`capture-2026-08-06`, `-2`, `-3`) — needs a
  scan of the directory to find the next free slot, races two processes
  against each other, and revives the numeric suffix that
  `adr/2026-07-id-collision-is-an-error.md` retired.
- **A random or hash id** — unique, and unreadable; the vault's ids are
  meant to be typed and recognized.
- **Sharing `Today` for the whole stamp** — a capture would then be named by
  the day the app launched, which for an app left open overnight is simply
  the wrong day.
