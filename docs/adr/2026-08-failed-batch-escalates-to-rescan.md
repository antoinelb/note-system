# A failed watcher batch degrades the liveness and escalates to a rescan

## Context

When applying a watcher batch to the index fails, the batch was consumed: the error became one overwritable notice and the skipped updates were never retried, so the index silently lacked them until an unrelated event touched the same files.
The watcher already has the right doctrine internally — lost events end the batch and emit `Rescan`, "once we know events have been lost, the following updates would apply to a state that can no longer be trusted" — but the rule was never applied around it.

## Decision

Taken with the user (2026-08-13), as part of the status-surface work (`adr/2026-08-status-surface-owns-notices.md`):

- **A failed `watch::apply` batch sets liveness to `Degraded` and queues a `Rescan`**, so the state self-heals and Degraded is usually transient; a later clean batch (or the rescan itself) restores `Watching`.
- **The cost is accepted knowingly**: a rescan today re-reads and re-parses the vault on the UI thread, so self-healing can stall a typing session until the compute-tier work (review candidate C3) moves scans off-thread.
  Visibility first, then convergence, then speed — but convergence was judged too cheap to defer behind a screen that merely renders the failure.

## Rejected

- **Render the degradation only, converge in a later phase** — honest layering and the reviewer's recommendation, but it ships a glyph that says "the index is behind" while the code knowingly leaves it behind.
- **Leave the batch consumed (status quo)** — contradicts the watcher's own trust doctrine and keeps "stale presented as current" as a designed outcome.
