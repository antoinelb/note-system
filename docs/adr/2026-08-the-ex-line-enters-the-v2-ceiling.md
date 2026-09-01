# The ex line crosses the v2 ceiling; the rest of that list stays out

## Context

`adr/2026-08-v2-caret-first-order.md` closed v2 with a *not in v2* list —
macros, marks, named registers, visual block, the ex line, the jumplist,
configuration — "the way v1's ceiling did". A ceiling is a decision about what
does not earn its way in *yet*, not a permanent refusal; v1's was crossed the
same way, one item at a time, when daily use asked.

Daily writing asked for one of the seven.

## Decision

**The ex line arrives** (`adr/2026-08-ex-line-is-literal-and-global.md`). It was
the cheapest item on the list — the `/` prompt already existed to reuse, and
`:s` is a demonstrated habit in the user's own config (`gdefault = true` is
only set by someone who substitutes often).

**The other six stay out**, with their reasons recorded now so the question is
not reopened from scratch:

- **Marks** — wanted (the user runs `vim-signature`), and the next candidate.
  Held back only because this batch was already large; they need a per-note
  store with its own lifetime rules beside the undo history.
- **Macros** — the grammar is pure over a `View`, so *recording* keystrokes is
  nearly free, but replay has to re-enter through an executor whose `Paste` and
  `WalkVisual` are spawned async. Ordering a replayed `@q` correctly is the
  real work, and it is not small.
- **Named registers** — contradicts `adr/2026-08-one-register-the-clipboard.md`
  head-on. That ADR's own terms ("a second register earns its way in through
  daily friction") still stand unmet: nothing in daily use has wanted one.
- **Visual block** — needs a third selection geometry the anchor cannot
  express, and prose has little use for a column.
- **The jumplist** — `Ctrl+O` belongs to the table
  (`adr/2026-08-jump-ctrl-o-centres-viewport.md`), and `Ctrl+I` is `Tab`, which
  is the indent verb (`adr/2026-08-tab-indents-in-every-mode.md`). Both keys are
  spoken for; the user's own nvim has the same `<C-i>` collision and lives with
  a one-directional jumplist.
- **Configuration** — the app is extended by editing the source
  (`adr/2026-07-buffer-is-path-plus-string.md`'s spirit, and CLAUDE.md's own
  framing). Nothing here has needed a knob that a recompile could not answer.

The ceiling is not deleted: it is now a list of six with reasons, which is more
useful than a list of seven without them.
