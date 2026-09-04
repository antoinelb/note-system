# An inactive block hides its syntax; the line under the caret is the source

## Context

`adr/2026-08-css-draws-the-markup.md` made every block draw the same `markup::model` spans whether it is active, selected or inactive, and it painted every span: the heading's `=`, the list's `-`, the quote's `> `, a checklist's `[ ]`, the `*`/`_`/backtick delimiters all stayed visible on every line.
The template renders none of them — a heading is its text, a task is a circle, a quote is a rule — so the editor showed the source everywhere and the paper never did.
The daily template's `- [ ]` under Tasks was the line that made this visible every morning.

## Decision

**Only `.block-css` hides.** The inactive block's `.mk-marker`, `.mk-delim` and `.mk-checkbox` spans get `font-size: 0`: still in the DOM, still tiling the source byte for byte, so the hit probe's `data-start` walk and the tiling property (`adr/2026-09-property-tests-guard-three-invariants.md`) are untouched — CSS stops painting, nothing stops existing.
The active block and the selected block keep every symbol: the line under the caret is the source, and a visual selection covers source.

**A marker owns the spaces after it.** `markup::model` now emits `"= "`, `"- "`, `"-   "` as one `Marker` span, the way the quote's `"> "` already was, and folds two adjacent marker spans into one; without this the hidden `-` left its space painted and every heading sat one space right of its paragraph.

**The replacement glyphs are `::before` content on the hidden span**: a list item's marker draws `•`, the first `[`/`]` span of a checklist draws `○` or `●` and the marker's bullet yields to it (`:has`), and every span after a done box is struck in the muted ink — the same three verdicts `templates/template.typ`'s `list.item` rule paints (`adr/2026-07-checklist-rendering.md`).

## Rejected

- **Emitting different text for an inactive block** (dropping the marker from the span) — breaks the byte tiling the hit probe and `j`/`k`'s goal-column walk read off `data-start`, and would mean two models for one source.
- **Hiding on the selected block too** — a selection highlights bytes; a highlight over a zero-width span is a selection the user cannot see.
- **A `Role` per marker kind** so an enum's `1.` keeps its number — no note in the vault writes an enum or a term item; both get the bullet until one does (the `ponytail:` line in `theme.css` names the split).

## Consequences

- Entering a line now moves its text right by the width of its prefix, and a long line may wrap differently active than inactive; the caret's own move is the only trigger, and the block box itself never changes height on its own.
- `#l(…)`, `#meta(…)` and every other call still show their source on inactive lines: a link's rendered text is the compiler's to decide, not a span CSS can hide.
- `adr/2026-08-css-draws-the-markup.md`'s "all blocks draw the same spans" still holds — what differs is only what the CSS paints of them.
