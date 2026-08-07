# The command palette floats centered — the app's first fixed overlay

## Context

The deck never drew a palette; phase 0 designs it in the phase-6-v0
vocabulary, like the loops list was (`adr/2026-08-loops-list-overlay.md`).
Both existing overlays (`.link-picker`, `.loops-list`) are in-flow bordered
boxes — `theme.css` has no `position: fixed` and no `z-index` anywhere. But
those overlays live where their subject lives: the picker under the block it
writes into, the loops list in the pane it describes. The palette's subject
is the whole app, and a summonable palette buried below a long note is
unreachable.

## Decision

- **Floating and centered** — the first `position: fixed`/`z-index` in
  `theme.css`, a deliberate break from the in-flow grammar, taken for this
  one overlay only. Metrics: `top: 96px` (clear of the chrome, near the
  top), `left: 50%` + `translateX(-50%)`, `width: 480px`, `padding: 8px`,
  `z-index: 1`, an opaque `var(--bg)` fill (it floats over the note) inside
  a `1px solid var(--border)` box. Every length a multiple of 4; every
  colour an existing custom property; no buttons.
- **The link-picker grammar, verbatim** (`adr/2026-08-ctrl-l-link-picker.md`):
  a `type-label` head reading "commands", the picker's own `.picker-query`
  input taking focus at mount, typing filters, arrows move the highlight and
  stop at both ends, Enter runs the highlighted row (nothing when nothing
  matches), Escape closes and restores focus, a click runs a row. Plain keys
  are `stop_propagation`'d; ctrl chords still bubble, so Ctrl+T works over
  an open palette. Selection is the rail's grammar: bold bright ink, no box,
  no fill. The chord, when a command has one, sits as a faint right-hand
  span — a hint, not a button.
- **Matching is the picker's case-insensitive substring** over the label
  (`links::filter`'s rule). Nine commands need no subsequence scoring.
- **No row cap.** The picker caps at `links::MAX_MATCHES` because the vault
  is unbounded; the registry is the app's whole bounded vocabulary, and
  seeing all of it is the point.
- **Ctrl+P lands on the logs pane's keydown**, beside Ctrl+L, because the
  dispatch needs the editor, the probe and every lifted callback — the root
  has none of them. The arm calls `prevent_default()`: WebKitGTK answers a
  bare Ctrl+P with its print dialog.
- **The caret is frozen at open**, the `Picker.anchor` idiom: when Ctrl+P
  bubbles up the textarea still holds focus and the probe answers; by
  dispatch time the palette's input owns focus and it would answer null.
  `insert link` and `follow link` run against the frozen offset.
- **Closing restores focus explicitly, two paths.** With no block active the
  pane's focus effect re-takes it (the effect now also reads the palette
  signal, and skips while the palette is up so it never fights the input's
  mount focus). With a block active, closing bumps the remount epoch with
  the frozen caret pending, and the textarea comes back focused with the
  caret where Ctrl+P found it. Enter-accept restores the same way, except
  for `insert link` (the link picker's input must keep the focus it just
  took) and `follow link` (the editor is replaced wholesale; a stale pending
  caret would leak into the next note's textarea).

## Rejected

- **An in-flow box like the loops list** — the native vocabulary, but a
  palette that scrolls with the note fails its one job: being reachable from
  anywhere.
- **A dimmed backdrop** — phase 3's writing-sheet vocabulary; one bordered
  box over the page is enough chrome for a list of nine lines.
- **The 8-row cap** — inherited from the picker, but capping a complete
  registry hides commands for symmetry's sake.
- **A subsequence (fuzzy-score) matcher** — over nine labels, substring is
  indistinguishable in use and has no ranking to explain.
- **Handling Ctrl+P at the app root** — symmetric with Ctrl+T/Ctrl+Q, but
  the dispatch would reach down for everything Shell owns; the Ctrl+L ADR
  already paid this argument out.
