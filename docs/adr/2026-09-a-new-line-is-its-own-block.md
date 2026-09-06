# A written newline ends the block it lands in

## Context

`= Titre`, Enter, and the fresh line was still a heading: the caret was heading-tall, heading-weight and heading-coloured, and anything typed into it stayed that way until the caret left the block.
`o` and `O` from normal mode did the same, and so did an Enter in the middle of a line.
A `> ` quote leaked its rule and its indent the same way; a list item leaked its indent.

The leak had one cause, and it is not in the renderer's arithmetic:

- `blocks::segment` makes a block one physical line, but `Editor::edit` deliberately does not reparse — *"No reparse — block boundaries move only on activate/deactivate"* (`src/editor.rs`), the typing path `2026-08-editor-splice-cross-block.md` named and defended.
  So between an Enter and the next activate/deactivate, one block holds two physical lines.
- The renderer models a block, once, and puts the verdict on the block's container: `let draw = markup::model(&text);` then `class: "block-active", class: if let Some(bc) = &block_class { "{bc}" }` (`src/ui.rs`).
  `markup::model("= Titre\nbody")` answers `BlockRole::Heading(1)` — the role is read off the block's *first* child — so the container is `mk-h1` and every `.source-line` inside it, the fresh one included, inherits `font-size` and `[class*="mk-h"]`'s weight and colour from `assets/theme.css`.

The rendered proof, from the failing test:

```html
<div class="block-active mk-h1">
  <div class="source-line"><span class="mk-marker">= </span><span class="mk-text">2026-07-23</span></div>
  <div class="source-line"><span class="caret"></span></div>
</div>
```

`BlockRole` is a property of one line. A container that holds two lines cannot carry one.

## Decision

**A write that carries a newline resegments on the spot.**
`Editor::resplit` rebuilds the block map and wakes the block owning the caret, and the two within-block write paths call it when — and only when — the text they wrote contains `\n`: `insert_at_caret` (typing's Enter, list continuation, `o`/`O`'s `Act::Type("\n")`, a multi-line paste, the IME's commit) and `splice`'s within-block route (a linewise `p` inside the active block).

- **`blocks::segment` stays the sole authority on what splits.** `resplit` does not count newlines; it re-runs the parse-tree walk. A newline inside a raw fence, a multi-line `#let`, a `#table` or display math is one child of typst's tree and merges exactly as it always did (`2026-08-per-line-block-segmentation.md`), so nothing that was one block for a reason stops being one.
- **It never flushes.** `deactivate` saves before it resegments; this runs mid-typing, so it only rebuilds the map and re-points `active`. Nothing reaches disk that the autosave was not already going to write.
- **Only a newline pays for it.** Every other keystroke keeps the typing path exactly as it was — no reparse, block boundaries fixed — so the cost is one note parse per Enter, not per character.
- **The role class stays on the block container.** It has to: `assets/theme.css` is explicit that the block box and the block-role class must sit on the same element, or a `font` shorthand on an inner wrapper beats the role's inherited `font-size`. Restoring the one-line invariant is what lets that stay true.

## What follows from it, beyond the caret's size

- The line above stops being active the moment Enter is pressed, so it hides its syntax like any other inactive line (`2026-09-inactive-blocks-hide-their-syntax.md`) — the same thing `j` off the line already did, now consistent with Enter.
- `ip`/`ap`, `dd` and everything else that reads the block map name one line after an Enter, not two.
- A mid-line Enter inside a list item still leaves one block, and should: `- une` + Enter + `idée` is `- une\n idée`, one `ListItem` in typst's tree and one list item on the page, so `mk-item` is the honest role for both lines. The rule is not "a newline splits", it is "a newline asks `blocks::segment` again".
- `Editor::caret_in_block` and every block-relative coordinate (`data-start`) are measured against the fresh line, which is why several tests that had encoded the merged block changed their expected numbers rather than their intent.

## Rejected

- **Per-line role classes on `.source-line` instead of the block container.** It would fix the appearance while leaving the block map wrong for `ip`/`ap` and `dd`, and it fights `theme.css`'s load-bearing rule that the role class rides the same element as the block box: `.mk-item`'s `padding-left` would stack on top of `.block-active`'s own `padding: 0 8px` and every nested item would step in 8px too far.
- **Resegmenting on every keystroke** — still rejected, for `2026-08-editor-splice-cross-block.md`'s reason: it is a parse per character for no gain. The rule here is narrower by exactly the thing that breaks the invariant.
- **Dropping the role class when a block holds more than one line.** The heading would shrink to body size the instant Enter was pressed — a worse lie than the one being fixed.
- **An e2e scenario.** The defect is markup, and `make test` renders markup: `ui::tests::a_line_opened_under_a_heading_draws_as_prose` asserts on the class the container carries for both openers the user named. `make e2e` asserts on files and the index and would not have seen it (`2026-08-headless-x11-e2e.md`).

## Amends

`2026-08-editor-splice-cross-block.md`, whose within-block route promised "no resegment — so a pasted blank line still splits at the *next* resegmentation, exactly like a typed one, and `o`/`O` semantics stay uniform".
The uniformity survives: a typed newline, a pasted one and `o`'s all resegment now, and all of them at the same moment. Only *when* changed — immediately, rather than at the next activate/deactivate.
