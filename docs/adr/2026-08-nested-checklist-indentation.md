# Nested checklist items indent 1em per level and never inherit a parent's strike

## Context

`adr/2026-07-checklist-rendering.md` matches a list item's leading children
against the `[ ]`/`[x]` bracket shape and joins everything after the
bracket into `rest`, which is struck when the parent is done. That rule
never accounted for nesting: typst does not wrap a nested `- [ ]` in a
child list — it lands as further `list.item` elements trailing the label
in the same body sequence, as siblings, not as a separate nested
`list`. `c.slice(3).join()` was swallowing those trailing items straight
into `rest`, so a nested item rendered with no indentation at all, and
a done parent struck its open children along with itself.

Typst exposes no nesting-level field on `list.item`, so there is no
number to read off the node; the only signal available is that a nested
item's `.func()` is `list.item` while the label's own children never are.

## Decision

- **The body's trailing children are split at the first `list.item`.**
  `tail.position(child => child.func() == list.item)` divides
  `c.slice(3)` into the label run (joined into `rest` exactly as
  before) and the nested run (untouched, never joined into `rest`, so
  `strike()` and the muted fill never reach it).
- **Each level indents by `1em`** via `block(inset: (left: 1em),
  nested.join())`, appended after the label inside the same outer
  `block`. `1em` at the note's `13.5pt` body size is `13.5pt` — enough
  to read as one step without being a second column.
- **No recursion of our own.** The nested run still contains
  `list.item` content, so typst re-applies this same `show list.item`
  rule to it when the content is laid out; a third level indents
  another `1em` inside the second level's `inset` block for free.
- **A done parent never mutes or strikes a nested child**, because the
  nested run is spliced in after `strike(rest)`/`text(fill: muted,
  ...)` has already been applied to the label alone.

Verified by rendering: a two-level checklist's nested task circles sit
`13.5pt` (`1em`) right of the top-level ones — `19.5pt` against the page
margin's `6pt` — and a done parent with one open nested child emits
exactly one strike-through path in the body (the parent's), confirmed
by walking the merged SVG's own `<g transform="translate(...)">` stack
rather than asserting on colour, since indentation, not colour, is the
point here.

## Rejected

- **Reading a nesting-level integer off `list.item`** — no such field
  exists on the node; the split has to be structural (is this child
  itself a `list.item`?), not numeric.
- **A recursive helper function that walks nested items itself** — typst
  already re-invokes the show rule on any `list.item` content it finds,
  including content we construct and re-emit; writing our own recursion
  would duplicate that and could disagree with it at a third level.
- **`pad(left: 1em)` instead of `block(inset: (left: 1em))`** — equivalent
  here since neither carries a background, but `inset` reads as "this is
  a nested block" rather than "shift this box", matching what the code
  is doing.
