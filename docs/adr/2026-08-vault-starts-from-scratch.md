# The vault starts from scratch — no migration

## Context

The app replaces Obsidian (daily notes) and a manual typst zettelkasten, and the old vault (`~/documents/notes`) still holds both in their pre-app layouts.
The first v1 roadmap draft made migrating that vault into `permanent/` a phase 0, on the argument that the table needs real notes to dogfood against.

## Decision

The vault starts from scratch.
Nothing is batch-migrated; the old vault stays where it is, untouched.

- **Fresh start on purpose**: the new system's conventions — `#meta` types, atomic permanent notes, `#l` links — should shape the notes from day one; old notes were written under a different model and don't fit it.
- **Cherry-pick over time**: an old note that turns out to be needed is rewritten by hand into the new vault at that moment, which is exactly the promotion gesture the system is built around — not batch-imported.

Consequence for v1: the table begins nearly empty and fills through in-app creation, which makes the CRUD phase the moment the vault starts growing, and means card density arrives organically rather than on day one (tests use the fixture vault for density).

## Alternatives rejected

- **Scripted batch migration** (ids, `#meta` synthesis, link rewriting) — produces a vault-sized pile of notes that were never written under the system's model, with guessed types and dates; the debt would be visible (typeless notes in the loops list) but not honest, since no one intends to pay it note by note.
- **Migrating only the daily-note history** — time notes gate nothing (the table excludes them), and the logs screen loses nothing by starting at the app's first day.
