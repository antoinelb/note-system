# Plain j/k walk the wrapped lines the webview draws, through a geometry seam

## Context

The vim friction batch item 8 gives plain `j`/`k` vim's real behaviour: stepping one *drawn* line at a time, not one logical (newline-delimited) line. Cormorant Garamond is proportional and the source pane wraps (`.block-active` is `white-space: pre-wrap`), so where a line actually breaks is geometry the grammar cannot see — `src/vim.rs` decides pure over a `View` snapshot of text and byte offsets, never the DOM. Item 7 already drew the boundary: `Act::WalkVisual { down, count, extend }` is what the grammar hands the executor, resolved until now by `walk_visual_fallback`'s logical-line walk alone (this ADR covers both).

## Decision

- **The executor resolves `WalkVisual` asynchronously**, the same shape `Act::Paste` already established: the grammar emits an intent, `apply_vim` `spawn`s a task, and only the task touches anything the grammar cannot see. The grammar stays pure and headlessly testable; the geometry lives entirely in `src/ui.rs` and `src/main.rs`.
- **A new seam, `LineProbe`, beside `HitProbe`**: `Fn(Option<f64>, bool, usize) -> Walk`, where `Walk` resolves to `Option<Landing>` — the landing span's `data-start`, its UTF-16 offset, the pixel x the walk held, and how many of the asked steps it actually took.
  `main` injects a JS probe that derives the step from `getComputedStyle` of the caret's `.source-line` line-height (falling back to the caret rect's own height) and walks `caretPositionFromPoint`/`caretRangeFromPoint` a step at a time — `HitProbe`'s own walk, aimed by geometry instead of a click.
- **The whole `[count]` run rides one round trip, and only its seed comes from the caret's drawn element.** `dioxus::document::eval` sends its script the moment it is constructed, so a step-per-eval walk would build the next probe in the same task poll as the previous landing, before the DOM flushed it, and read the caret's pre-move rect.
  The script therefore reads `.block-active .caret, .block-active .caret-box`'s `getBoundingClientRect()` once, to start the run, and every later step measures the rect of the character the previous step landed on.
  The loop is bounded twice: `bounded_steps` clamps the count Rust-side to the note's characters + 1 before anything walks, and the JS loop breaks again as soon as a landing stops changing.
- **The seam takes no coordinate from the caller.** `HitProbe` takes client coordinates because a click carries its own; `WalkVisual` has none — the caret is always drawn in the active block, and `data-start` is block-relative, so seeding from the caret's own rect needs nothing else from the caller.
- **The probe returns the pixel x it used.** A run that has no goal yet passes `None` in, so the probe resolves one from the caret's own rect and hands it back out — the goal column bootstraps in the same round trip that moves the caret, rather than a separate resolve-then-walk step.
- **The goal column lives in the component, beside `dragging`/`probing`**: a `Cell<Goal>`, where `Goal { x, column, generation }` holds the seam's pixel x, the logical cluster column the degraded fallback keeps, and a stamp.
  Both columns are held across a run and forgotten together — by every key that is not a walk (count digits included), by every key the grammar swallows, and by every mouse-driven caret move — exactly as vim's own goal column is.
  `generation` disowns a walk that resolves after its run was already forgotten: the task compares the stamp it started with before touching the caret or the column.
- **A probe with nothing left to answer degrades to the logical-line walk for the rest of the count.** No seam injected at all, a miss, the caret already on the note's first or last drawn line, or a landing outside the active block all read as `None` and take the same fallback — `walk_visual_fallback`, the logical walk from item 7, which is also what crosses into the neighbouring block and clamps at the note's ends.
  The seam reports how many steps it took, and the remaining `count - taken` go through one `motions::motion` call, so a reported miss and an absent seam are identical at any count (rendered side by side and compared) and `3j` sitting on a block's last drawn line moves three lines rather than one.
- **Operator-pending `j`/`k` and the arrow keys keep logical lines** — untouched by this seam, per the batch's own scope: `dj`/`yk`/`cj` stay vim's linewise behaviour, and insert mode's arrows stay phase 0's.

## Rejected

- **Passing a note-global byte into the probe** — considered and dropped in favour of the caret's own rect: the caret is always drawn exactly once, in the active block, so there is nothing a byte would add that the DOM does not already carry, and skipping it means the probe needs no coordinate translation at all.
- **Asking the probe once per step, a bounded Rust loop around the seam** — the second step's origin is the first step's landing, and `dioxus::document::eval` sends its script at construction time, so every step after the first would measure a rect the DOM had not yet flushed; moving the loop inside the webview, where each step measures the character it just landed on, is what makes the run correct, not merely cheaper.
