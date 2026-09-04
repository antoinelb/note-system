# One font size for the editor's source and the rendered SVG

## Context

`.block-active` and the compiled fragment sitting beside it were both
"18px" on paper — `.block-active { font: 18px "Cormorant Garamond" }`
against `template.typ`'s `set text(size: 13.5pt)`, the same number at 1px =
0.75pt. They still disagreed on screen, because `.note svg { max-width:
100%; height: auto }` scaled the compiled page (a fixed 14cm ≈ 529px) down
to fit whenever `.centre` was narrower than that — which was most window
widths, since `.centre` was a bare flex child with no width of its own. The
editor's textarea never scaled. Two faces that matched only at one exact
window width is not parity, and widening or narrowing the window visibly
pulled the rendered text away from the source line above or below it.

## Decision

- **The SVG stops shrinking to fit.** `.note svg` renders at its intrinsic
  size (`max-width: none`); a new `.centre-column` wrapper inside `.centre`
  caps the reading measure at 529px (the compiled page's own width) and
  centres it, and `.centre` itself gains `overflow-x: auto` so a window
  narrower than that scrolls sideways instead of shrinking the type.
  (Amended twice: the column went fluid below its cap in
  `adr/2026-08-css-draws-the-markup.md`, so a narrow window shrinks the
  column rather than scrolling; and the logs' cap is 720px since
  `adr/2026-09-the-logs-column-is-720px.md`. The clause this ADR is about
  — the *type* never shrinks to fit — stands in both.)
- **One token, read twice.** `--prose-size: 18px` and `--prose-leading:
  1.5` live on `:root` in `assets/theme.css`; `.block-active`, `.block-
  pending .pending-source`, `.block-selected .selected-source`, and the
  `min-height` of `.source-line`/`.block-blank` (`calc(var(--prose-size) *
  var(--prose-leading))`) all read it instead of a literal `18px`/`1.5`.
  The `.logs` pane overrides the token from a signal, so the whole subtree
  moves together.
- **The template reads the same number as a compile input.** `template.typ`
  now does `sys.inputs.at("size", default: 13.5)` and derives the body,
  heading (`× 24/13.5`) and meta (`× 6.75/13.5`) sizes from it, keeping the
  ratios `adr/2026-07-reading-scale-bumped.md` set. Vanilla typst (`make
  check-vault`, exports) gets the default, unchanged.
- **`RenderTheme` carries the size alongside the colour column**:
  `Paper(u16)` / `Dark(u16)` / `Light(u16)`. The three frozen `LazyLock`
  libraries become a mutex-guarded memo keyed by `(colour, size)`, since the
  library is otherwise immutable and the app only ever asks for a handful
  of pairs; a poisoned lock degrades to a fresh build rather than
  panicking. The size is part of `RenderTheme`'s `Hash`, so it already
  rides the fragment/body cache keys without extra plumbing — a size change
  invalidates exactly like a theme change does.
- **`Shell` gets one `font_size: Signal<u16>`**, default 18, folded into the
  `RenderTheme` built for every fragment and body compile. No control reads
  or writes it yet; the settings page (todo 16) adds one.

## Rejected

- **Scaling the SVG via CSS to match a chosen font size** — the mirror
  image of the bug this fixes: it zooms the paper instead of changing the
  type scale, so the source pane (drawn at a fixed px size) would disagree
  with it again the moment the two numbers diverged. This is the same
  alternative `adr/2026-07-reading-scale-bumped.md` rejected for the
  original mismatch; the difference here is the mechanism doing the
  scaling was `.note svg`'s `max-width: 100%`, not a deliberate CSS zoom —
  removing it, not adding another one, is the fix.
- **A separate `--source-size` token for the textarea** — the entire point
  is one number; two tokens kept in sync by convention is the bug in a new
  shape.
- **Passing size as a second, independent World/render function argument**
  instead of folding it into `RenderTheme` — every cache key, `FragmentJob`,
  and `BodyJob` already carries `RenderTheme` end to end; adding a parallel
  `size: u16` beside it at every call site duplicates exactly what the enum
  already threads through the compute tier.
