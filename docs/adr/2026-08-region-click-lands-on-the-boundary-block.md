# A click anywhere in a compiled region activates the block next to the active line, not the block under the pointer

**Superseded by `adr/2026-08-css-draws-the-markup.md`**: compiled regions are gone — one widget per block again, whether CSS-drawn or Typst-compiled — so a click now lands on the block it actually hit, not the one bordering the active line. Placing the caret at the exact byte offset inside a clicked, non-active block remains unimplemented; a click still only activates the block, same coarse resolution as before.

## Context

ADR `2026-08-cursor-split-rendering` merges every block above (and below) the active line into one compiled Typst fragment per side. Before that change, one fragment existed per block, so a click carried its own block's identity for free — the fragment a click landed in was the block to activate. A merged region has no such per-line seam once it compiles: `typst-svg` hands back one SVG for the whole region, with no surviving per-source-line marker a click's pixel position could be resolved against.

`region_pane` (`src/ui.rs`) still has to answer *some* block for a click anywhere in the region, because clicking rendered text is how the active line moves in this editor. It picks the block adjacent to the active line's edge of the region (`activate_at`) for every click in that region, regardless of where in the region the pointer landed.

## Decision

**A click anywhere in a compiled region activates the one block bordering the active line, not the block the pointer's pixel actually sits over.** This is a deliberate narrowing from the per-block precision the old one-fragment-per-line model gave for free — accepted because recovering it needs a source-to-pixel mapping this app has no layer for (typst-syntax gives byte spans, `typst-svg` gives paint geometry, and nothing bridges the two once several blocks share one compiled fragment).

## Alternatives rejected

- **SyncTeX-style position mapping** — annotate the Typst source with per-line markers and recover their painted position from the compiled output, the way VS Code's LaTeX Workshop syncs a PDF back to source. Real engineering: a second coordinate system to build and keep in sync, for a click precision gap the two-fragment model's own ADR already accepted as unaddressed performance/porting work.
- **One fragment per block again, restored just for hit-testing** — recompiling per line to recover per-line click targets undoes the whole point of `2026-08-cursor-split-rendering` (the per-line compile cost and the lost cross-line nesting it was written to fix).
- **Binary-search the click's pixel row against line count** — approximate a line index from the click's vertical offset inside the region and the region's total rendered height. Rejected: Typst's own layout (font metrics, paragraph spacing, list indentation, wrapped lines) makes "row height" non-uniform, so the guess is wrong exactly where it matters — near a wrapped or nested line.

## Consequences

A click far from the active line still moves the active line only one block closer, not straight to the clicked one — a user chasing a distant line by mouse takes it in more than one click, or uses a motion instead. This degrades pointing precision, not correctness: `activate_at` always resolves to a real, in-bounds block, and the deficit is confined to the two compiled regions — the active line itself, still a real `<textarea>`, keeps exact byte-level click resolution.
