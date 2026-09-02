# `NOTE_TODAY` pins the app's clock, for the e2e harness and nothing else

## Context

`adr/2026-07-today-injected-root-context.md` made `time::today()` the one
clock read and rejected an env-var override: the headless UI tests inject
a fixed date through root context, and an env fallback would be a test
seam nothing below `main` needs. That holds for `make test`. `make e2e`
drives the shipped binary, which has no root context to inject through,
and every scenario computed `today=$(date +%Y-%m-%d)` on its own — a run
straddling midnight would write `time/<yesterday>.typ` and poll for
`time/<today>.typ` until it failed, and the fixture vault's July 2026 rail
sat beside whatever the wall clock said.

## Decision

**`time::today()` reads `NOTE_TODAY` first, at the same edge `NOTE_VAULT`
crosses**, and falls back to the clock. The read stays a single one in
`main`; nothing below it changes. A value the parser refuses is not a
request to stop the clock — the same rule `vault::vault_path` applies to
a bare `NOTE_VAULT=` — and the harness's own file oracles catch a pin
that did not take, since the day note it polls for is named after the pin.

**`harness.sh` pins `E2E_TODAY=2026-07-24`** and exports it as
`NOTE_TODAY` when launching the binary. The fixture vault lives in July
2026 and ships no note for that day, so "today" is empty on every run and
the rail around it is the same rail every time. Scenarios name the day
note as `time/$E2E_TODAY.typ`; none reads `date` any more.

The `today()` unit test that raced midnight (`today() == now()`) is
replaced by one that brackets the call between two clock reads.

## Alternatives rejected

- **Reading `date` in the harness and exporting that** — removes the
  midnight race but keeps the moving rail; a fixed day gives a fixed
  screen for the persona sessions too.
- **A `--today` flag** — the same seam with a second spelling; the app
  already speaks to its environment through one prefix.
- **Faking the clock with `faketime`** — an `LD_PRELOAD` over WebKitGTK
  and a Typst compiler, for one date the app reads once.
