# `Editor::splice`: one entry point, two routes

## Context

A phase-3 change can span the whole note (`dG`, `dj` on a block's last line), but the editor's one edit op was block-scoped `edit` (`adr/2026-07-hybrid-active-block-textarea.md`). `Buffer::replace_range` already spans the note; the open decision was the `Editor` entry point's shape (`roadmap-v2.md` § Phase 3).

## Decision

`Editor::splice(span, replacement, caret)` — note-global bytes in, a post-splice caret coordinate to land on.

- **Within the active block's content**, it routes through `edit`: the typing path, no resegment — so a pasted blank line still splits at the *next* resegmentation, exactly like a typed one, and `o`/`O` semantics stay uniform.
- **Crossing the block**, it splices the buffer, resegments (deactivation's mechanism), wakes the block owning `caret` and places it — the flush-and-resegment path everything else already takes.
- Linewise spans include each line's trailing newline **only when it is block content**: the newline after a block's final line belongs to the separator, so `dd` there deletes the line and leaves the bare separator to merge at the next resegmentation — no separator arithmetic anywhere (`motions::linewise_span`).
- Every refusal is the loud `STALE_EDIT` notice, the one staleness policy.

## Rejected

- **Always splice-and-resegment** — immediate resegmentation on every keystroke-sized change would make a typed blank line split instantly, changing the v0 editing feel phase 0 promised to keep.
- **A second block-scoped entry beside `edit`** — the routing rule is one comparison; two public write paths with different staleness stories was the thing v0 refused.
