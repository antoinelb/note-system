# The debt counter opens a flat list

## Context

The design pass (`docs/design/ui-spec-table-and-logs.md`, since superseded by `wireframes-v0.md`) scopes the friction system down to almost nothing: "open loops are out of scope: they appear as a counter in the corner and nothing more".
Wireframe `3a` shows it as a bare `6` in the top bar.

That conflicts with the v0 exit criteria, which require "capture notes + open-loops panel".
Meanwhile phase 3 already shipped and tested the two queries the panel needs: `typeless_notes()` and `dangling_links()`.

## Decision

The top bar carries the counter, on both screens, and **clicking it opens a flat list** of the same items.
No ages, no grouping by kind, no per-item actions — the list is a destination, not a workspace.

The expensive part of open-loops was the index queries, and they are done.
What remains is rendering two `Vec<PathBuf>`, so cutting the list saves close to nothing while leaving the user a number they cannot act on.
The full panel the plan describes (ages, grouping, suggestion debt) stays v1+, as the design intended.

## Alternatives rejected

- **Counter only, per the design** — a notification with no inbox; the number tells you debt exists and then refuses to say which.
- **The full panel with ages and grouping** — the design cut it deliberately, and ages need a "when did this capture arrive" policy that phase 9 has not decided yet (file mtime lies after any edit).
