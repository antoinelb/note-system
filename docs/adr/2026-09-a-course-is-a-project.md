# A course is a `project`; the `course` type is removed

## Context

`adr/2026-09-course-type-and-due-loops.md` added `course` as the ninth permanent type, with its own template, hue and picker row.
Two days of use showed it carried nothing a `project` did not: a course has a start, an end and a goal, and the lectures link it exactly as they would link a project.
What outlives the course — the concepts, the claims, the sources — was never in the course note; it lives in the permanent notes the course's notes link to.

## Decision

**The `course` type is removed and the closed set is eight again**: `NoteType::Course`, `templates/course.typ`, `--type-course` and `.card.bar-course` are gone from `create::TYPES`, `is_permanent`, the type bars and the filter overlay.
A course is a `project` note; a lecture is still a `source` linking it, and the knowledge a course produces is the permanent notes that outlast it.
A note still carrying `type: "course"` reads as `NoteType::Unknown`: it keeps its row and its `type` column, wears the untyped bar and owes no loop; the user retypes it to `project` by hand.

The `due` half of the earlier ADR is untouched: `due` is a `#meta` field any note may carry and the loops list reads it. The `course-and-due` e2e scenario becomes `due-loops`.

## Alternatives rejected

- **Keep `course` as an alias of `project` in `from_name`** — an alias is a type the picker never offers but the parser accepts, a second spelling for one thing; the repo has no other alias and the one migration is a handful of notes.
- **A migration that rewrites `type: "course"` to `type: "project"` in the vault** — the app never rewrites prose or `#meta` on its own (CLAUDE.md's AI-never-writes invariant is the same instinct), and the untyped bar makes the leftovers visible.
