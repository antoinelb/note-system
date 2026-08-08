# Screen switching: chrome icons, palette commands, Ctrl+1 / Ctrl+2

## Context

v1 phase 2 mounts `Screen::Table`, so for the first time there are two screens to move between.
Neither the wireframes nor the plan name the gesture: the deck draws the two chrome icons ("a button each way", `adr/2026-07-two-screens-table-and-logs.md`) and forbids keyboard hints in the chrome, while phase 0's rule demands every command be reachable by name (`adr/2026-08-palette-birth-command-list.md`).

## Decision

**Three gestures, one seam: clicking a chrome icon, the palette commands "go to table" / "go to logs", and the chords Ctrl+1 / Ctrl+2 — all running the same `go_table` / `go_logs` callbacks.**

- The chords read as screen ordinals in chrome-icon order: 1 is the table, 2 the logs.
- The palette hides the screen already stood on (`Context::on_table`) — going where you stand is not a command; the chord for it simply lands nowhere, the `InsertLink` guard idiom.
- The chords live in each pane's keydown, not the app root: they follow Ctrl+P's precedent, and the pane is what holds focus.
- Leaving the logs closes the active block and the link picker the way Escape would: their textarea and input are about to unmount, and a hidden overlay waiting behind a screen would reopen unasked on the way back.
- Each pane requests focus in its own `onmounted` — the unmounting pane takes focus with it, `autofocus` fires at document load only, and chords only arrive by bubbling from inside the app.
- The command palette moves from inside the logs' centre section to the shell level, beside the panes, so one instance floats over whichever screen is up.

## Alternatives rejected

- **A `Routable` router** — two variants with no URLs, no history and no deep links do not earn a router; a `Signal<Screen>` and a match is the whole feature.
- **Icons only** — leaves the switch mouse-only and breaks the palette's completeness-by-construction rule.
- **A single toggle chord (Ctrl+Tab style)** — one chord fewer, but the palette needs the two named directions anyway, and a toggle's meaning depends on where you already are.
