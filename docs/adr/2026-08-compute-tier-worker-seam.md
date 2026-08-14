# The compute tier: one seam between the UI thread and everything slow

## Context

The AIR review (C3) found the app had no compute tier: typst compiles ran
inside the render diff (the frame that first needed a pixel paid the whole
compile, plus a system font scan on the first ever), the startup scan and
rebuild ran before anything painted, and every watcher batch ran SQLite on
the UI thread.
A theme toggle or a zoom to Bodies froze the window for the duration.

## Decision

Taken with the user (2026-08-13):

- **A `compute` module owns the seam**: `Job` (fragment, body, survey) in
  through `ComputeFeed::submit`, `Outcome` back on one channel a shell
  task drains into the caches and signals — the watcher bridge's shape,
  pointed the other way. `compute::run` is the one executor; an adapter
  decides *where* it runs, never *what*.
- **Two adapters make the seam real.** `threaded` (production): two worker
  lanes, one for compiles and one for surveys, so a zoom's flood of body
  compiles never delays a watcher batch — each lane one thread, FIFO,
  which is what keeps batches applying in arrival order. `inline` (the
  headless default when no feed is injected): a job runs at submit time,
  so the ~650 existing tests stay deterministic and the first render is
  complete; a scripted third adapter in the tests holds jobs for the
  pending-state assertions.
- **The drain repaints once per burst** (`recv_many`), bumping one tick
  signal the shell reads — the caches stay plain memo stores.
- The watcher loop thins to "invalidate bodies, forward the batch as a
  survey job"; the failed-batch escalation
  (adr/2026-08-failed-batch-escalates-to-rescan.md) moves to the drain,
  bounded by an `escalated` flag the job itself carries.
- The font scan moves off-thread for free: the `FONTS` static is first
  touched wherever the first compile runs.

## Rejected

- **`spawn_blocking` / a thread pool per call site** — N copies of the
  dispatch-and-land plumbing, and no single place to keep batch ordering.
- **Making the caches signals** — every fill would repaint every reader;
  the tick keeps repaint policy in one place.
- **One worker lane for everything** — a bodies zoom queues dozens of
  compiles ahead of the watcher's survey; index freshness should never
  wait on pixels.
- **Async all the way down in tests** — real threads in the harness make
  ~650 assertions racy; the inline adapter keeps the seam while keeping
  determinism.
