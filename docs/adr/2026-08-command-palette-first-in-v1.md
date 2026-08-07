# A command palette, and it goes first in v1

## Context

v1's order was fixed dogfooding-first in `adr/2026-08-v1-walking-skeleton-order.md`: eight phases, positions store first.
Meanwhile every command the app has lives only as a keyboard chord (theme toggle, capture, loops list, link picker, time movement), discoverable only by knowing it — and Obsidian's Ctrl+P palette was part of the daily workflow this app replaces.
v1 is also the version that adds the most new keystrokes (create, filters, zoom, arrange), each an undiscoverable chord if nothing names it.

## Decision

A command palette — Ctrl+P, fuzzy search over named commands, built on the link-picker overlay pattern — becomes v1 phase 0, ahead of the positions store; the existing phases shift to 1–8.
Going first buys a rule the rest of v1 inherits: every phase that adds a keystroke adds its palette entry in the same change, so the palette is complete by construction, never by audit.
Ctrl+P is unbound today; no chord moves.

## Alternatives rejected

- **v0 polish backlog** — it is a feature with its own design surface (overlay, command registry, tests), not polish; the backlog is for friction found while daily-driving what exists.
- **Late in v1, once the commands exist** — cheapest per command, but then all of v1 is built without it and naming the accumulated chords becomes exactly the retrofit audit the phase-by-phase rule avoids.
- **Extending the ceiling silently** — v1's list is the ceiling (`plan.md` § Known risks 4); this ADR is the explicit, user-decided exception that keeps the ceiling meaningful.
