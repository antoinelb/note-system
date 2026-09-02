# A changed fallback block keeps its last SVG while the recompile is out

## Context

`adr/2026-08-async-caches-pending-stale.md` gave bodies a stale-while-revalidate shelf and fragments none: "the key contains the content, so a result is valid for its key forever" — and a changed block therefore had no previous compile to hold in its place, so it dropped to dimmed source until the tier answered.
Since `adr/2026-08-css-draws-the-markup.md` the only fragments left are the blocks CSS cannot draw — equations, tables, figures, images — and an equation is exactly the block a student edits in small steps, watching it.
Every keystroke inside `$…$` flashed the formula to raw source and back; on a page of them the flicker was the editor's most visible latency.

## Decision

**`FragmentCache` shelves the last good SVG per block slot.**
`probe` now takes the block's index in the note; a `Ready(Ok)` answer records the slot's key, and a probe that misses answers `Pending { job, shelved }` with the SVG the slot last showed.
`sweep` keeps every shelved entry of the note last probed — an active block probes nothing across the sweeps its edits cause, and that is exactly when its shelf must survive — and drops every other note's shelves with the generation, so the bound is the open note's block count.
The editor draws a shelved image in the block's slot with the `block-stale` class — an opacity, not a colour — and the raw dimmed source only when the slot never showed an image.
The fresh compile replaces it when it lands, exactly as before.

**Errors are never shelved**, as for bodies: a block that failed shows its error, and the good image before it stays the shelf for the next change.
A template clear forgets every shelf with every entry.

**The key is still content-addressed.** Nothing about validity changed — a result is still right for its key forever; what changed is only what the slot shows while the next key is out.
The ADR of 2026-08 carries a note pointing here.

## Alternatives rejected

- **Compiling equations on the UI thread** — the whole point of the tier (`adr/2026-08-compute-tier-worker-seam.md`) was that the frame never pays for a compile.
- **Keying fragments by slot instead of content** — a cursor move would then recompile the block it left and the one it entered; content keys are why moving costs nothing.
- **Shelving the error too** — the 2026-08 ADR's reason holds: the typo must be seen where the block is.
- **A debounce before submitting the compile** — later pixels, not fewer flashes; the shelf removes the flash without delaying the answer.
