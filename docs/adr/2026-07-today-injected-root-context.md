# "Today" enters the UI as an injected root-context value

## Context

The logs screen needs today's date for its initial selection, the month
grid's starting month and the lit season — many more readers than the old
Today button.
The app's rule is one clock read at one edge; the headless UI tests need
that edge deterministic (the fixture vault lives in July 2026).

## Decision

- `time::today()` is the single clock read (moved from `ui.rs`).
- `main` injects the value as `ui::Today(Date)` through root context — the
  same channel as `VaultRoot` and `Closer`
  (`adr/2026-07-ui-covered-at-100.md` blesses `insert_any_root_context`).
- Tests insert a fixed date (2026-07-23) instead; nothing below `main`
  reads the clock.

## Rejected

- **Calling `today()` inside components**: every test would depend on the
  wall clock, and the fixture vault would need regenerating forever.
- **An env-var override**: the ui-covered ADR already rejected env
  fallbacks for test seams.
