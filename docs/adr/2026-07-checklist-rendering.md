# `- [ ]` and `- [x]` render as task circles

## Context

The daily template seeds a Tasks section with `- [ ]`, the Obsidian habit
the vault inherits — but typst rendered the brackets literally. In typst
markup, `[` and `]` are plain text, so the item body's leading children are
the texts `[`, `]` around a space or an `x`.

## Decision

- `template.typ § note()` gains a `show list.item` rule: an item whose
  leading children are `[`, space-or-`x`, `]` becomes a task row — an open
  stroked circle, or a `done`-filled circle with a white check and the rest
  of the line struck in the muted ink. Each palette column carries its own
  `done` green. The row is emitted as plain content, not a rebuilt
  `list.item`, so the bullet marker vanishes and the circle takes its
  place; every other list item is untouched, as are brackets in prose.
- Matching reads the children's `text` fields (content equality works for
  `[ ]`/`[x]` blocks but not for escaped-bracket literals).
- **Rendering only**: ticking a box is editing — activate the block and
  type the `x`. Click-to-toggle on the rendered SVG would need
  source↔layout hit-mapping and an accept-writes-to-notes story; if it
  ever matters it is its own decision.

## Rejected

- **The `cheq` package** — the vault refuses packages by design
  (`adr/2026-07-embedded-typst-world.md`); the rule is ~15 lines.
- **A `#todo(..)` template function** — abandons the `- [ ]` muscle memory
  and makes capture syntax heavier than plain text.
