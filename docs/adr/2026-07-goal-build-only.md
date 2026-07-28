# Project goal: build the note system, drop the learning-Rust goal

## Context

Until now (2026-07-27) the project carried a dual goal: build the note system *and* have Antoine learn Rust/Dioxus by writing all production code himself, with Claude limited to tests and guidance.
That split was encoded in `CLAUDE.md` (roles paragraph) and `roadmap-v0.md` ("How we work", `*(Claude)*` tags on every test task).

## Decision

The learning goal is dropped.
The project's only goal is building the note system; whoever is at the keyboard writes whatever gets it done — code, tests, docs.

Consequences applied:

- `CLAUDE.md`: the roles paragraph removed (including "tests after production code", which existed to serve the learning loop).
- `roadmap-v0.md`: "How we work" rewritten without role assignments; `*(Claude)*` tags removed.
- Earlier ADRs mentioning the learning goal (e.g. `2026-07-v0-walking-skeleton-order.md`) stay as written — they are the historical record, and the phase ordering they justify still holds on its dependency arguments alone.

## Alternatives rejected

- **Keep the role split without the learning framing** — the split only existed to force hands-on learning; without that goal it is pure overhead.
- **Rewrite old ADRs to remove learning mentions** — ADRs record why decisions were made at the time; editing history would defeat their purpose.
