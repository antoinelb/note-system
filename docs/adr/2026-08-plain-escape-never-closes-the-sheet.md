# Plain Escape stops closing the sheet; only Shift+Escape does, even with no block active

## Context

`adr/2026-08-shift-escape-leaves-the-note.md` made Shift+Escape the one way a note (and, on the table, its sheet) is left, and made plain Escape inert *while a block is active* — the sink swallows it before it can bubble to the pane. That ADR does not cover the table's own rung: `table_keys` carried its own arm, `Key::Escape if sheet.peek().is_some() => close_sheet.call(())`, which fired independently of any block, for the case of a plain Escape reaching the sheet's own chrome (its footer, its card) with no textarea focused at all. That rung closed the sheet on a bare Escape the same reflex press the earlier ADR calls out as meaningless — "the key you press to be sure of the mode" — undoing its own guarantee the moment the caret was not literally inside a block.

## Decision

The chrome rung is deleted. `table_keys`' plain-Escape arm now only acknowledges a visible notice, mirroring the logs pane's arm exactly; only the `event.modifiers().shift()` arm above it closes the sheet, whether or not a block happens to be active underneath. Leaving a sheet is now one gesture, one guarantee, regardless of what has focus inside it — the same reflex-safety `adr/2026-08-shift-escape-leaves-the-note.md` established for the block case now holds for the chrome case too.

## Rejected

- **Leave the chrome rung and only guard the block-active case** — the inconsistency this ADR removes: two different Escape behaviours depending on pixel-level focus state inside the same sheet, neither documented as intentional.
- **Fold this into `adr/2026-08-shift-escape-leaves-the-note.md` as an edit** — that ADR's decision was already shipped and referenced elsewhere; this is a distinct rung removed in a later round, and gets its own record rather than a silent rewrite of a settled one.
