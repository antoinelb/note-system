# The anomalies the index writes are read back as open loops

## Context

The `anomalies` table was INSERT-only: a malformed `#meta` indexed as
defaults and its recorded anomaly was never shown anywhere — a dead
observability channel (the AIR review's C6).
Worse, `parse_note`'s node cap silently stopped the walk: links — even
the `#meta` itself — beyond 100 000 nodes were simply never seen, with
nothing recorded at all.

## Decision

Taken with the user (2026-08-13):

- **`Index::anomalies()` reads the table back**, one row per (note,
  family), where the family is already the loops-list label: the parser's
  recorded meta anomalies fold into "malformed meta", a capped walk is
  "truncated".
- **Anomalous notes join the open-loops list** as more debt kinds beside
  typeless, dangling and still-open — the app's existing visible-debt
  channel, so no new chrome and no new vocabulary of severities. The
  ember counts them like any loop.
- **A truncated parse records itself**: `parse_note` reports whether the
  walk exhausted its cap, the note carries the flag, and the index writes
  it as a note-level anomaly row — note-level because a truncated walk
  may have missed the `#meta` the other anomalies hang off.

## Rejected

- **An indexed-at / generation stamp** (the speculative half of the
  original candidate) — a schema bump with no consumer; add it when
  something reads it.
- **One loops line per anomaly row** — a note with five malformed fields
  is one problem, not five; the family fold keeps the ember honest about
  how many notes need a hand.
- **Raising `MAX_NODES` instead of recording the truncation** — any cap
  silently exceeded is the same bug at a different size.
