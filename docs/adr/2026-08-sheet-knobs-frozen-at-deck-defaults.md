# The sheet's knobs freeze at the deck defaults

> Superseded in part by `adr/2026-09-the-sheet-is-an-index-card.md`: daily-driving supplied the feel signal this ADR froze without, and `SHEET_LEFT`/`SHEET_WIDTH` are gone — the sheet is a 5:3 card sized from the pane by `table::sheet_frame`.
> `--dim-opacity: 0.4` still stands.

## Context

Phase 3 left `sheetW` and `dimOpacity` as the deck's open knobs — "pick by feel once it runs, then freeze".
The app has run at the deck defaults since (sheet at x=440, 620 wide; dim 0.4), but daily-driving only starts at the end of phase 4, so no feel signal exists to pick from.

## Decision

Freeze the defaults as they stand: `SHEET_LEFT: 440`, `SHEET_WIDTH: 620` (`table.rs`), `--dim-opacity: 0.4` (`theme.css`, both themes).
If daily-driving grates, each is a one-line change and this ADR gets superseded — freezing costs nothing and closes the phase.

## Rejected

- **Holding phase 3 open until a feel pass happens** — a phase kept open for a signal that cannot arrive before phase 4 blocks the roadmap on nothing.
- **Guessing different values now** — a change without a feel signal is noise pretending to be design.
