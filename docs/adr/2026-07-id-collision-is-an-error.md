# An id collision at creation is an error, never a numeric suffix

## Context

`adr/2026-07-id-scheme-kebab-frozen.md` (phase 1) said "collisions at creation get a numeric suffix (`-2`)".
Phase 5's creation code is the first to implement collision handling, and implementing the suffix surfaced what it actually does: it silently accepts a duplicate title.

In this vault two notes with the same title are almost certainly either the same note (write in the existing one) or a naming mistake (pick a better title).
A silent `deep-modules-2.typ` hides that from the one person who could resolve it, and because ids are frozen at creation, the misleading id then outlives any later title fix.
The system's friction philosophy is the opposite: problems become visible debt, they are not papered over.

For time notes a suffix is worse than misleading — `2026-07-27-2` would corrupt the axis the logs screen sorts on, so the suffix path could never be shared code anyway.

## Decision

**A note whose id already exists is a creation error (`TemplateError::AlreadyExists`), reported to the user; no suffix is ever minted.**

Consequences:

- One public `create` function serves every note type: time notes pass the date as the title, and kebab-case leaves `2026-07-27` unchanged — the planned `create_exact` twin is not needed.
- `NoFreeId` and the bounded suffix-retry loop disappear before being written.
- The user resolves a collision by choosing a different title (or opening the existing note); the app never chooses for them.

This supersedes the collision sentence of `adr/2026-07-id-scheme-kebab-frozen.md`; the rest of that ADR (kebab ids, frozen at creation, filename = id) stands.

## Alternatives rejected

- **Numeric suffix (`-2`), as originally decided** — resolves the collision without user involvement, which is exactly the problem: it converts a naming mistake into two near-identical ids that autocomplete and backlinks then propagate.
- **Prompt to merge/rename at creation time** — better UX someday, but it is a UI flow, not a storage rule; the storage layer erroring cleanly is what makes such a flow possible later.
