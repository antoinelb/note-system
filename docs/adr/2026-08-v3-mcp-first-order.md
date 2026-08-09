# v3 orders itself MCP-first: the vault answers before it suggests

## Context

v3 is the AI version (`plan.md` § AI integration): two separate mechanisms — the app shelling out to the `claude` CLI for suggestions, and an MCP server exposing the vault to any Claude Code session — plus the surfaces that make suggestions ambient (dashed edges, the end-of-page line, ghost text) and the tag-confirm flow.
Each prior version opened with an ordering bet recorded in an ADR: v0 walking-skeleton, v1 palette-first, v2 caret-first.
The pieces of v3 have real dependency structure: ghost text needs v2's owned-caret widget (the uncontrolled textarea cannot draw it), the surfaces need a store to read, the engine needs a store to write, and the MCP server needs nothing but the vault on disk.

## Decision

`roadmap-v3.md` orders v3 as: MCP server → suggestion store → engine → ambient surfaces → ghost text → tags.

- The MCP server goes first: it is the smallest phase, touches no UI, follows the `--capture` second-process pattern verbatim (`adr/2026-08-capture-headless-second-process.md`), delivers standing value from day one — every Claude Code session gains the vault as context — and forces the repo's first serialization-dependency decision while the stakes are lowest.
- The store precedes the engine, and the engine precedes every surface: data before writers, writers before readers — each phase leaves something daily-usable (debt lines count even before edges dash).
- Ghost text is placed after the surfaces and explicitly rides v2 phase 0; if v3 somehow runs first, that one phase waits rather than forcing a throwaway textarea overlay.
- A *not in v3* list (generated-note pipeline, ambient triggers, vault-wide sweeps, MCP write tools, embeddings, ranking UI) closes the version the way v1's and v2's ceilings did.

## Rejected

- **Engine first (risk-first)** — the biggest unknown is whether `claude` suggests good links, and v0 tradition attacks icebergs early; but the spike still needs the store to land results in, so "engine first" really means "store then engine" — which is this order minus the cheap standing value of the MCP phase.
- **Surfaces first, on fixture data** — dashed edges and the page line could be built against hand-written rows, decoupling UI from prompt quality; rejected because it polishes chrome for data that may never materialize in that shape, and the loops list already gives the store a visible surface for free.
