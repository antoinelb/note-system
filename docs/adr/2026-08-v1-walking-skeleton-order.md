# v1 phase order: dogfood before spectacle

## Context

`plan.md` § Roadmap fixes v1's contents (the table) and § Known risks 4 fixes its ceiling; neither fixes an order.
v0's order (`adr/2026-07-v0-walking-skeleton-order.md`) was a walking skeleton with daily-driving pulled as early as possible — phase 5 of 10 — and the bet paid: every later phase was tested by real use the day it landed.

Two facts constrain v1 specifically:

- The vault starts from scratch (`adr/2026-08-vault-starts-from-scratch.md`) — the table begins nearly empty, so in-app creation is the only way permanent notes come to exist, and nothing can be daily-driven before CRUD lands.
- Everything on the table sits on positions, whose durability is a named invariant (`adr/2026-07-positions-separate-file.md`, decided in v0 with implementation deferred to now).

## Decision

Eight phases, dogfooding-first (breakdown in `roadmap-v1.md`):

0. positions store — the invariant as tested code, before any UI consumes it
1. table at rest — cards, kinds, pan, drag (wireframe state 6a)
2. writing sheet — state 6b, reusing the v0 editor
3. permanent CRUD — **daily-driving starts here**, phase 3 of 7
4. link edges
5. semantic zoom (bodies)
6. filters + jump-to-note
7. auto-placement + on-demand cluster arrange

Editing and creation (2–3) come before edges, zoom and findability (4–6) because v1's purpose is to retire the manual zettelkasten workflow, the from-scratch vault cannot grow at all until creation exists, and only real daily use queues the right polish — v0's backlog proved that mechanism works.
Auto-placement is last: until then "unplaced" has a dumb honest fallback, and the placement heuristic wants a table already full of hand-placed cards to be judged against.

## Alternatives rejected

- **Visuals first (edges and zoom before the sheet)** — demo value without workflow value; nothing daily-drives a read-only constellation, and on a from-scratch vault there is nothing to draw until creation exists anyway.
- **A migration phase 0** — rejected in `adr/2026-08-vault-starts-from-scratch.md`; the vault fills through use, not import.
- **Positions folded into the table phase** — the durability invariant deserves its own tested phase before UI concerns blur it, the same reason v0 phase 3 tested "delete `.index/`, everything comes back" before any screen existed.
