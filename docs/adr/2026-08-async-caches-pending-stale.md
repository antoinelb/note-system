# Probing caches: pending shows dimmed source, stale bodies hold the slot

> Amended by `adr/2026-09-fragments-shelve-their-last-svg-per-block.md`: the
> "fragments need no staleness guard" bullet below still holds for validity —
> a result is right for its content key forever — but a fragment slot now
> keeps its last good SVG on screen, dimmed, while a changed block's new
> compile is out, instead of dropping to dimmed source.

## Context

With compiles on the compute tier (adr/2026-08-compute-tier-worker-seam.md),
the render caches can no longer compile on a miss — a probe must answer
*something* for a compile that has not landed, and an outcome can land
after the file it compiled has changed.

## Decision

Taken with the user (2026-08-13):

- **The caches answer `Ready(result)` or `Pending`**, handing back the job
  to submit exactly once — an in-flight set dedups the repaints between
  queue and landing. The synchronous `render` path stays beside `probe`
  for the inline adapter.
- **A pending block shows its raw source, dimmed** — the source⇄rendered
  model the app already speaks, and the note is readable instantly. A
  pending card body with nothing to show is a quiet gap.
- **Bodies keep a stale shelf**: invalidation moves the last good SVG
  aside and the slot keeps showing it while the recompile is out —
  stale-while-revalidate. Errors are never shelved; showing them is the
  point.
- **A failed compile's error replaces the slot**, as before — seeing the
  typo is the feature, and it matches the cached-error-as-value house
  pattern. Last-known-good is kept only while pending, not after failure.
- **Fragments need no staleness guard** — the key contains the content, so
  a result is valid for its key forever. **Bodies are path-keyed**, so the
  cache carries an epoch: invalidation bumps it, and an outcome queued
  under an older epoch is dropped whole (its in-flight slot was already
  cleared, so the next probe re-queues against the current epoch). One
  global epoch per cache — a bump discards unrelated in-flight compiles
  too, which simply re-queue; per-key epochs if that ever thrashes.

## Rejected

- **Showing the other theme's SVG while a theme toggle recompiles** — a
  sibling-key lookup for a rare gesture; dimmed source is honest and free.
- **Stale SVG kept beside a failed compile** (the strict LAT-6 reading) —
  the error must be seen where the block is, and two values per key buys
  chrome for a state the editor should surface, not soften.
- **Content hashes for body validity** — a disk read per probe; the epoch
  is one integer and the watcher already says when to doubt.
