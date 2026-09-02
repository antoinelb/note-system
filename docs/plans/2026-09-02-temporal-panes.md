# Temporal panes: a narrower rail and a way to fold both side panes

Done 2026-09-02 (`adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md`), as item 8 of `2026-09-02-tier-3-student-workflow.md`.

## Goal
The logs screen's side panes cost no more width than they need, and the keyboard can fold them.

## Context
`todo.md` held three temporal-pane items; this plan replaces it.
The first — the note's width never exceeding what is available — landed with the fluid reading column `min(529px, 100%)` (`adr/2026-08-css-draws-the-markup.md`) and is dropped.

## Out of scope
Any change to what the rail lists (`adr/2026-07-rail-continuous-newest-first.md`).

## Constraints
Invoke the `air` skill first; no colour literals outside `assets/theme.css`; spacing in multiples of 4.
Alt currently reaches the grammar only as AltGr's carrier and is swallowed inert (`adr/2026-08-escape-ladder-editor-wide-mode.md`); binding Alt+H/Alt+L must not break composed characters in insert mode.
A folded pane is session-only state, like the settings overlay's knobs (`adr/2026-08-settings-overlay.md`).
Every decision gets an ADR; `make test` stays at 100%; the fold gets an e2e scenario.

## Items
1. `.rail` (`assets/theme.css`, `width: 208px`) narrows to the widest date row plus one 16px gutter each side; measure against the fixture vault's longest row.
2. Alt+H folds the rail and Alt+L folds the jump panel, both toggles, both reachable from the palette; the folded pane leaves a hairline so the fold is visible.

## Acceptance
`make static && make test` green at 100%; a scenario folds both panes and proves typing still lands in the note.

## Check
`make static && make test`
