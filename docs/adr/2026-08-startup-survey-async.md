# Startup is the first survey: the shell paints before the index exists

## Context

`load` ran `scan_vault` plus a full index rebuild inside a `use_hook`
before anything painted — a blank window for O(vault), and one
`IndexError` replaced the whole app with the vault-error screen even
though the `.typ` files were intact (the ERR-5 all-or-nothing the AIR
review flagged).

## Decision

Taken with the user (2026-08-13):

- **Startup submits `Survey([Rescan])` through the compute seam** — the
  same job a watcher batch or an escalation is. Under the threaded adapter
  the shell mounts with empty lists and the survey lands like any batch;
  under the inline adapter the mount surveys synchronously, which is
  exactly the old launch (and what keeps the headless tests' first render
  complete).
- **The vault-error takeover survives only for "no vault root"** — the one
  failure with nothing to paint. An index that will not build is a
  critical-side notice and a Degraded liveness glyph; the app stays up,
  rail and table empty until a rescan succeeds.
- **Today's note opens by a stat, not the survey** — the file is the
  truth, and the threaded launch has no survey yet to consult.
- The survey ensures `.index/` exists, but only inside a vault that does —
  a mistyped root fails the survey instead of being silently created and
  surveyed as empty. `Positions::save` ensures its own parent for the same
  window: the debounce can fire before the first survey creates `.index/`.

## Rejected

- **The vault-error takeover, delivered late** — the app flashing alive
  and then dying, and the same failure wearing two faces depending on
  whether it happens at launch or mid-session.
- **Blocking the first paint on the survey under the threaded adapter** —
  that is the freeze this whole decision removes.
- **Opening today's note only after the survey lands** — the editor is the
  daily driver; a stat costs nothing and works on an unindexed vault.
