# Enter continues a list marker

## Context

The daily template seeds `- [ ]` under Tasks (`tests/fixtures/vault/templates/daily.typ`), and the shared template renders `- [ ]` and `- [x]` as task circles (`adr/2026-07-checklist-rendering.md`).
Every following item's marker is retyped by hand, at the right indentation.

Enter already rewrites the line it ends: `> quote` expands to a native `#quote` (`adr/2026-08-greater-than-expands-to-quote.md`), which established both the gate — a collapsed caret at a physical line's end — and the splice that keeps the rewrite inside the current insert intent.

## Decision

- **Four markers continue: `-`, `+`, `- [ ]`, `- [x]`.**
  Typst wants a space after the marker, so `-abc` is prose while a bare `-` closing the line is an empty item — the shape the daily template seeds, which must continue.
- **A done item opens a fresh empty one**: `- [x] écrit` + Enter gives `- [ ] `.
  No item is born already checked.
- **The indentation is carried, not recomputed** — the new item sits at exactly the leading whitespace of the one above it.
- **An empty item walks one level out per press, and at the margin leaves a bare line.**
  `    - ` becomes `  - `, then `- `, then nothing at all — marker *and* indentation gone, the caret at column 0.
  A list therefore ends with one Enter per level of nesting, and the last press leaves no orphan alignment spaces for a following paragraph to inherit.
  These presses insert no newline: an empty item ends the list where it stands rather than pushing a blank line ahead of it.
- **The gate is the quote expansion's own**: a collapsed caret at the end of a physical line.
  Enter anywhere else in an item splits it plainly, as it does today.
- **The quote shorthand is tried first.** `> ` is neither marker, so the two cannot both match; the order is fixed anyway so a future shorthand cannot silently reorder them.
- **Every rewrite goes through `Editor::splice`**, so the whole continuation belongs to the current insert intent and one `u` reverses it with the surrounding typing.

**Amended 2026-09-01, after `adr/2026-08-greater-than-is-the-stored-quote.md`:** the Enter-time quote expansion the gate and the ordering above were defined against is deleted — `>` is now stored literally and read by a `show par:` rule in the template, not rewritten on Enter. The list-marker gate (a collapsed caret at a physical line's end) stands on its own and needs no borrowed justification from a mechanism that no longer runs; "the quote shorthand is tried first" no longer describes live code, because there is nothing left at Enter time for list continuation to be ordered against.

## Rejected

- **Continuing from mid-item** (Obsidian's behaviour: Enter splits and carries the marker to the tail) — a second gate to reason about, where the quote expansion's line-end rule already reads clearly and covers the way lists are actually written.
- **Keeping the indentation when the marker is cleared** — leaves invisible trailing spaces on what looks like an empty line, which the next paragraph then inherits.
- **Clearing an empty nested item in one press** — loses the level-by-level walk out of a nested list, which is the only way to return to the margin without arrow keys.
- **Recognising `1.` and other explicit Typst enum numbering** — no daily-writing need has surfaced; `+` auto-numbers, which is what gets typed.
