# Repository language: everything in the repo is English

## Context

The repo grew up bilingual: `plan.md`, the roadmap and most ADRs in English, two ADRs and the fixture vault in French, and UI strings undecided until `wireframes-v0.md` pinned them ("UI strings are english; note content keeps its own language").
The mix had a real cost: French was guessed as the UI language from the surrounding ADRs, against the spec.

## Decision

Every artifact **in the repo** is English: ADRs (content and filenames), fixture notes (content, filenames — which are note ids — and tags), templates, code comments, commit messages, UI strings.

What this does *not* cover: the **real vault's content** stays in whatever language the user writes — that remains the wireframes rule, and fixtures may deliberately reintroduce non-English ids later where the id scheme needs the coverage (`2026-été` in phase 5).
References to files in *other* repos keep their real names (two French ADR paths from `generateur_horaire` cited in `2026-07-coverage-100-percent-lines.md`).

Applied in one pass: 17 ADRs and 8 fixtures renamed with `git mv`, every reference rewritten (docs, makefile, source, tests, including vault-internal `#l` targets and the `evergreen-notes` dangling id), the 2 French ADR bodies and 13 French fixture bodies translated.
Two test expectation lists changed because renames changed `ORDER BY path` results — nothing else in the suite moved.

## Alternatives rejected

- **Translate content, keep French filenames** — smallest diff, but the repo stays half-French forever at exactly the names cited most often (ADR references, fixture ids in tests).
- **Keep fixtures French as a realistic mirror of the real vault** — fixtures are test fodder, not vault samples; their ids leak into test assertions, and realism about content language is not a property any test relies on.
