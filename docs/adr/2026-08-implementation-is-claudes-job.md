# Implementation is Claude's job

## Context

`adr/2026-07-goal-build-only.md` dropped the learning-Rust goal and left roles neutral: "whoever is at the keyboard writes whatever gets it done."
Since then Claude has written the implementation in practice, and Antoine confirmed (2026-08-13) that implementing is no longer his job — the neutral phrasing no longer describes how the project works.

## Decision

Claude writes the implementation — code, tests, docs.
Antoine directs: sets goals, takes the design decisions, reviews, and uses the app.
The per-task loop (discuss approach → ADRs → implement with tests → commit) is unchanged; only the hands on the implement step are now named.

Consequences applied:

- The v0 How we work section: the "whoever is at the keyboard" clause replaced with the explicit roles; roadmaps v1–v3 inherit it through their "the v0 loop, unchanged" reference.
- `CLAUDE.md`: one line under Other instructions stating the role, so every session starts with it.

## Alternatives rejected

- **Keep the neutral phrasing** — it existed to erase a role split that served the dropped learning goal; now that the roles have settled the other way, neutrality only hides who does what.
- **Rewrite older ADRs that mention Antoine writing code** — same reasoning as in `2026-07-goal-build-only.md`: ADRs are the historical record, and editing history defeats their purpose.
