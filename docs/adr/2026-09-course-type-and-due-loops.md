# `course` is the ninth permanent type, and `due` is a `#meta` field the loops list reads

## Context

The user is a university student, and the vault had no shape for a course: a course was a `project` by convention, a lecture a `source`, and the assignment deadlines that make up most of a term's debt lived in prose where nothing could count them.
The open-loops list knows four kinds of debt — typeless notes, dangling links, unsummarized captures, unreadable `#meta` — and `adr/2026-07-debt-counter-then-list.md` closed its vocabulary: "no ages, no grouping by kind, no per-item actions".
The 2026-09-02 audit put a course workflow in tier 3; the shape was decided with the user the same day.

## Decision

**One new permanent type, `course`**, last in `create::TYPES` and in every enumeration the closed set of eight had (`is_permanent`, the type bars, the filter overlay): nine now.
Its template is a title and two prose lines, `Code:` and `Term:` — no new placeholders (`adr/2026-07-template-placeholders-closed-set.md`).
**A lecture is a `source` note** that links its course with `#l`; the course's backlinks are its lecture list, and no `lecture` type was needed to get it.
`--type-course` is derived by the rule `adr/2026-08-light-table-colours-derived.md` set for the other eight: a muted green on dark (`#6f9a7a`), the same hue with more chroma on light (`#3f8f5c`).

**`due: "YYYY-MM-DD"` is a `#meta` field on any note.**
The parser reads it as it reads `created` — a string that parses as a date, an unparseable one its own `InvalidDue` anomaly, a non-string a malformed field — and the index stores it in a `due` column (schema version 4, so the index rebuilds rather than migrates, `adr/2026-07-disposable-index-user-version.md`).
`templates/template.typ`'s `meta` accepts it and prints `due 2026-09-10` in the meta line, so a note compiled by the vanilla CLI says the same thing the loops list says.

**Two loop families, last in the list**: `Index::due_notes(today)` returns every note due on or before today plus seven days, soonest first, and `loops::lines` names each `<id> · overdue since <date>` when the day has passed and `<id> · due <date>` otherwise.
The line opens the note like every other loop line.
This amends the "no ages" clause of the 2026-07 ADR by exactly one word: a date is spoken where the date *is* the debt.
There is still no grouping and no action.

**`today` rides in the survey job.** `Job::Survey` carries the shell's injected date, and `compute::survey` hands it to the due read; the compute tier never reads the clock itself (`adr/2026-07-today-injected-root-context.md`).

## Alternatives rejected

- **A `lecture` type with a template that prefills its course** — Ctrl+N's two steps have no third for "which course", and a `source` note with one `#l` is what a lecture already was.
- **Inline `#due("date")` calls, one loop per call** — every deadline is a note's deadline in practice; a call inside prose would want the parser to walk every function call for a second name, and a note with three calls would owe three loops for one piece of work.
- **A `due` family that lists everything dated, however far** — a loop is debt now; a deadline in December is a fact, not a loop. Seven days is the horizon a week's planning needs.
- **A `course:` field on lecture notes instead of `#l`** — a link is what backlinks, the table's edges and Ctrl+Enter already understand.
