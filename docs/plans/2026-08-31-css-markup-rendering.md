# CSS markup rendering, and `>` as the stored quote

## Goal

The editor renders note markup as styled HTML spans laid out by the browser, with the embedded Typst compiler kept only for constructs CSS cannot draw, so that moving the cursor compiles nothing and reflows nothing.
A note renders identically whichever screen it was opened from, so a time note in the logs and a permanent note in the sheet behave the same.

## Out of scope

The table's card bodies keep compiling whole notes to SVG through `BodyCache` — they have no cursor, no editing, and no metric to agree with.
`blocks::segment` and the block grain are unchanged: a block is still one physical line, and `dd`, `ip`/`ap` and every motion keep their meaning.
The vim grammar, the index, the watcher, the palette and the canvas are untouched.
No vault migration: existing `#quote(...)` notes keep rendering, because the template keeps its `show quote.where(block: true)` rule alongside the new one.
The uncommitted `make e2e` harness in the main tree is not part of this worktree and is not wired in here.

## Constraints

Rendering coverage is decided by the parse tree, never guessed: a block whose `typst-syntax` tree holds only markup nodes plus the calls the app owns (`#meta`, `#l`, `#quote`) renders as CSS; anything else — `Equation`, `#table`, `#figure`, `#image`, any unrecognised `FuncCall` or `Code` node — falls back to a compiled SVG widget.
Every note still compiles standalone with the vanilla typst CLI, and `make check-vault` still passes: the `>` form is taught to Typst by a `show par:` rule in `templates/template.typ`, not by preprocessing.
No colour literal appears outside `assets/theme.css`; every markup role is a custom property, filled in for both themes.
All UI spacing is a multiple of 4.
Comments are never prefixed with `ponytail: `.
No while loops; production code avoids `expect`.
`make test` returns to 100% regions, lines and functions.

The active block, `.block-active`, is a `div` of drawn `Piece` spans and not a real `<textarea>`, so styling it is possible; the app draws its own caret and selection, and those must stay byte-exact inside styled spans.

## Items

1. A markup model derived from the `typst-syntax` tree: one pure function turning a block's source into styled spans carrying a role and a block-relative byte range, plus the verdict "CSS can draw this" or "this needs Typst", with the verdict decided by node kind alone.
2. Inactive blocks render from that model as styled spans instead of a compiled SVG region, keeping the `data-start` offsets that click-to-activate and the mouse hit probe already depend on.
3. The active block renders styled too, with its markup delimiters visible at their rendered weight, while `caret::layout`'s caret, selection and IME preview pieces stay byte-exact and the line's metrics do not change on entry or exit.
4. Blocks under a visual selection render with the same roles, so a line entering or leaving the selection still shifts nothing below it.
5. The SVG fallback becomes one widget per block, cached by content, so that a cursor move recompiles nothing; the cursor split, `Side`, and the per-side stale shelf retire with it.
6. `>` becomes the stored quote syntax: a `show par:` rule in `templates/template.typ` teaches vanilla Typst to read it, and the Enter-time expansion in `src/editor.rs` is deleted.
7. The markup roles get their colours and weights in `assets/theme.css` as custom properties, both themes filled in together.
8. The two editor hosts are reconciled onto one fluid reading column, `min(529px, 100%)`, so a note wraps at the same measure on both screens and the editor is fully visible at any window width instead of scrolling sideways.
Today `.centre-column` caps the logs editor at a fixed `max-width: 529px` (the compiled page's 14cm) while the sheet has no reading column at all and gives the editor its own content box of 554px — `SHEET_WIDTH: 620` less `padding: 24px 32px` and a 1px border — so the same note wraps 25px later in the sheet than in the logs.
Going fluid is what CSS rendering makes free: rewrapping prose costs nothing once no SVG has to agree with it, and this does not reopen `adr/2026-08-one-font-size-for-source-and-render.md`, which rejected shrinking the *type* to fit a narrow pane, not the column.
The SVG fallback widgets of item 5 keep their compiled width and scroll inside their own `overflow-x: auto` container rather than forcing the column wider.
9. The decision records: supersede `2026-08-greater-than-expands-to-quote.md`, add one for CSS-decoration rendering that supersedes the rendering half of `2026-08-cursor-split-rendering.md` and records the accepted editor/export divergence, and update CLAUDE.md's Rendering bullet and load-bearing invariants to match.
10. Coverage returns to 100%: tests that asserted compiled-SVG panes are retired or rewritten against the styled spans, the new markup model is covered directly, and one test asserts the logs screen and the sheet render the same note to the same spans at the same measure.

## Acceptance

`make static && make test` is green with coverage at 100%.
`make check-vault` still compiles every fixture note with the vanilla typst CLI, including a note written with the new `>` quote form.
The user runs the app on a long note and judges vertical navigation by feel: holding `j` must not lag behind the caret, and prose above and below the caret must not reflow as it moves.
The same note opened as a time note in the logs and as a permanent note in the sheet wraps at the same measure and renders the same, judged in the running app.
With the window at half the screen (960px of 1920), the editor is fully visible on both screens with no sideways scrolling of prose.

## Check

`make static && make test`
