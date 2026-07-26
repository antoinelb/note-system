# One `time/` category instead of `daily/` and `weekly/`

## Context

The plan's storage layout had two time-based directories (`daily/`, `weekly/`), and season notes were coming, which would have meant a third.
Meanwhile the load-bearing invariant says the note *type* is a `#meta` field, not a directory.

## Decision

A single `time/` category directory holds all time-based notes (daily, weekly, season, …).
Which kind a note is lives in its `#meta` type, like everywhere else; the index, not the filesystem, answers "all weekly notes".
The four categories become: `time/`, `permanent/`, `capture/`, `generated/`.

## Alternatives rejected

- **One directory per time kind (`daily/`, `weekly/`, `season/`)** — encodes type in the filesystem, contradicting the type-is-meta invariant, and grows a directory per new cadence.
- **Subdirectories (`time/daily/`, …)** — same contradiction, one level deeper.
