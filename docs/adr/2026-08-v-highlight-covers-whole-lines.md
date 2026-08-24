# V paints whole lines

## Context

Visual mode drew line-wise selections at their raw byte ends, not full lines — noted as friction-backlog at the time (`adr/2026-08-visual-selection-is-the-anchor.md`). The vim friction batch item 3 closes it.

## Decision

- **The widening lives in `caret::layout`**, behind a plain `linewise: bool` appended to its signature — not a `vim::VisualKind`. `caret.rs` keeps no dependency on `vim`; the widget passes `matches!(vim.read().mode, vim::Mode::Visual(vim::VisualKind::Line))` in from `ui.rs`. `v` (and every non-`V` caller) passes `false` and stays byte-exact, unchanged.
- **The drawn extent is whole lines, first character to last, plus one trailing newline cell per covered line** — not one cell for the whole selection.
  Each fully-covered line gets its own `Piece::Selected` cell at its own end, and that cell is a plain space (U+0020), *not* the no-break space the box caret uses as its own line-end stand-in.
  The line renders under `white-space: pre-wrap`, where a preserved trailing space hangs rather than counting towards the line box, so it can never widen the line; a no-break space can, since it neither breaks nor hangs, and `V` applies the cell to every covered line at once — a paragraph already filling the pane would visibly reflow on mode entry.
  A line whose end coincides with `head` skips the extra cell — the box caret's own stand-in (still `"\u{a0}"`, in `push_caret`) already draws there, so nothing doubles.
- **The widened extent matches what `motions::linewise_span` (src/motions.rs:415) will actually cut, except on a block's final line**: both derive a line's start as the byte after the previous newline and its end as the next newline or the source's end.
  On a block's last line, `linewise_span` stops at `line.end` — the newline after it belongs to the block separator, not the line — while `build_line` still paints that line's trailing cell, matching vim's own convention of always drawing one more cell than a linewise cut removes.
  `caret::layout`'s doc comment names the divergence; the highlight is not lying about the cut everywhere else, but it is not a byte-exact preview of it there.
- **`v` is unchanged**: `layout`'s `selection` stays the raw `anchor.min(head)..anchor.max(head)` when `linewise` is `false`, and no trailing cell is ever appended.

## Rejected

- **A `VisualKind` parameter on `caret::layout`** — pulls `vim` into `caret.rs`, breaking the module's headless, mode-agnostic contract for one bool's worth of information.
- **One trailing cell for the whole selection instead of one per line** — does not match vim's own visual-line rendering (every covered line highlights through its own newline), and would under-draw a wrapped multi-line highlight.
