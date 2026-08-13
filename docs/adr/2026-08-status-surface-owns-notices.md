# The status surface: one module owns every notice and the liveness fact

## Context

An architecture review through `docs/interface-rules.md` (AIR) found that the app's entire error surface was `Editor.notice: Option<String>` — single-slot, severity-less, overwritten by any later tick — while a real degraded mode (the watcher failing to start) announced itself only on stderr.
Every subsystem (autosave, positions, watcher batches, delete, capture) competed for the one slot, and ~10 `ui.rs` call sites each re-invented `map_err(|err| format!("…{err:?}"))`.
The wireframes allow almost no chrome: "the only chrome in the whole app is a single top line and one number."

## Decision

Taken with the user (2026-08-13):

- **A `status` module owns every user-facing message and the liveness fact** (`Watching | Unwatched | Degraded`); nothing else displays a failure.
- **`Editor.notice` is deleted.**
  Editor operations surface trouble as returned values (a diverged widget edit, a failed flush), and the shell reports them to status — the editor returns results instead of holding a display concern.
- **Placement**: the chrome line gains one small liveness glyph beside the ember — dim when Watching, bright when Unwatched or Degraded, the same place in every state.
  Notices keep rendering on the existing in-pane notice line; the full history sits behind a palette command whose overlay reuses the loops-overlay pattern.
  This is a deliberate, minimal amendment to the wireframes' "one line and one number".
- **Three severities, per AIR ALR-2**: info yields to any later notice; warning persists until dismissed or resolved; critical persists until acknowledged or resolved, and the highest severity wins the line.
- **Acknowledgement is Escape at the bottom of the escape ladder** — Escape acknowledges the visible critical only when no block, sheet, or overlay is open above it; one new bottom rung, no new key, no button.
- **Resolution beats gestures**: a later clean save clears its own failed-save critical; a healthy batch clears a degraded warning — the condition ending is not an auto-dismiss.
- Message prose lives behind the interface in one place ("source: what happened — what to do now"); the watcher-start error rides the existing `VaultFeed` into the UI instead of dying on stderr.
- Not applicable by project rules: FR/EN parity (all UI strings are English) and copyable diagnostic IDs (single user, no support channel; the history overlay carries the full text).
- **One deliberate exception**: the create overlay's refusal message stays overlay-local (`adr/2026-08-ctrl-n-two-step-create-overlay.md`) — it is inline validation shown where the user is typing, and the overlay occludes the notice line; the time-note create path, which has no overlay, reports through status.

## Rejected

- **In-pane only, no chrome change** — zero wireframe cost, but an unwatched vault is then only visible until the next notice overwrites the line; "quietly frozen in the past" returns as a state.
- **A full status segment on the chrome line** (liveness plus latest notice text) — words in chrome the design language deliberately stripped of words.
- **The editor keeping its intrinsic notices** (stale-edit, flush failure) — smaller diff, but two notice surfaces permanently, and a rule needed for who wins the line.
- **Two severities (info / trouble)** — the reviewer's recommendation; the user chose the full ALR-2 ladder for headroom and to keep the acknowledgement class distinct from ordinary trouble.
- **A palette-only dismissal** — acknowledging an alert is not destructive; the summon-and-name friction belongs to delete, not to reading a message.
