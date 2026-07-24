# `created` is a parsed date (jiff), not a raw string

## Context

`#meta` carries a `created: "YYYY-MM-DD"` field.
Phase 2 had to decide whether the domain type stores it as written or parses it into a real calendar date.

## Decision

Parse `created` into `jiff::civil::Date`.
A value that does not parse becomes `None` plus a recorded anomaly (see `2026-07-anomalies-meta-donnees-fines.md`).
Library: **jiff** — modern API, first-class plain calendar dates (`civil::Date`), better parse errors than the alternatives.

## Alternatives rejected

- **Raw `String`** — zero dependencies, but every later consumer (sorting by date, capture age in the open-loops panel) would re-parse and re-handle malformed dates independently; validating once at the boundary is the defensive default.
- **chrono** — the incumbent; larger API surface, older design, no advantage here.
- **time** — lighter than chrono but a clunkier parsing API.
