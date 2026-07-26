# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A personal Typst knowledge system: a Linux desktop app built with Dioxus, storing every note as a plain `.typ` file compilable by the vanilla typst CLI.
It replaces Obsidian (daily notes) and a manual typst zettelkasten.
Single user, no accounts, no plugins — extended by editing the source.
This project has a dual goal: create this new knowledge system and all the user to learn rust and Dioxus.
As such, your responsibility are unit tests and guiding, and the user should write all actual code.

## Documents

- `docs/plan.md` — **the standalone source of truth**: design principles, note model, storage layout, screens, editor, canvas, friction system, AI integration, roadmap, risks. Read it before writing code.
- `docs/design/` — UI direction: the wireframe deck plus `wireframes-v0.md`, its full description — Part I is the operative spec (two screens: the table, v1; the logs, v0 — layout, palette, chrome, states), Part II the decision trail. Read Part I before writing any UI.
- `docs/adr/` — decision records (see below).

## Decision records (ADR) — required practice

Every decision taken from now on is documented in its own file under `docs/adr/`:

- One decision per file, kebab-case name (e.g. `2026-07-positions-separate-file.md`).
- Keep it short: context, the decision, alternatives rejected and why.
- When a decision changes the plan, update `docs/plan.md` *and* add the ADR — the plan carries the **what**, the ADR preserves the **why**.
- When you (Claude) participate in a decision with the user, write the ADR as part of the same change; do not let decisions live only in conversation.
- When committing there's a decision that's unclear, ask the user why something was made the way it was to document it in an ADR.

## Stack and architecture (decided)

Rust throughout — details and reasoning in `docs/plan.md`:

- **UI**: Dioxus 0.7 desktop. **Whenever Dioxus code is written or understood, first read `.claude/dioxus.md`** (Dioxus 0.7 API reference): 0.7 changed every API — `cx`, `Scope`, and `use_state` are gone; use `use_signal`, `#[component]`, `rsx!`, `Routable`, `use_resource`.
- **Rendering**: the typst compiler embedded as a Rust crate — compile note → SVG, cache per note, invalidate on edit.
- **Index**: SQLite under `vault/.index/` (links, tags, positions, suggestions), rebuilt by parsing files, kept live by a file watcher.
- **Parsing**: extracting `#meta` and `#l` calls uses the `typst-syntax` crate — a real parse, never regex.

## Load-bearing invariants (constraints, not preferences)

- **Plain files are the source of truth.** The index is derived and must always be rebuildable from the `.typ` files; every note compiles standalone via the shared `template.typ`.
- **AI never writes prose in note files** (sole exception: the explicit `generated` type). Suggestions live only in the sidecar index; accepting a link suggestion = the user writes it (Tab-inserted ghost text), never an accept button that edits the file.
- **No hard blocks.** All friction (unsummarized captures, unresolved suggestions, dangling links, typeless notes) is visible debt in the open-loops panel, never a save-blocker.
- **Canvas positions are user data disguised as index data** — they must survive index rebuilds.
- **Strict buffer/widget separation in the editor**, so the v2 vim modal layer can be inserted without a rewrite.
- Note **type is a `#meta` field, not a directory**: directories encode only the four categories (`permanent/`, `time/`, `capture/`, `generated/`); the index, not the filesystem, is the authority for querying by type.

## Roadmap

Four versions (`docs/plan.md` § Roadmap):
- **v0 — daily driver for writing**: vault structure + `#meta`/`#l` conventions, file CRUD from per-type templates, daily notes, hybrid block editor (fallback: a single pane toggling source ⇄ rendered), the design language (palette + type scale as theme variables, dark and light), the logs screen, link index + backlinks + dangling-link detection, capture notes + open-loops panel.
- **v1 — the table**: canvas with persistent positions, semantic zoom, modal card editing, filters, auto-placement. The v1 list is the ceiling, not the floor.
- **v2 — vim**: modal editing layer on the existing buffer architecture.
- **v3 — AI**: `claude` CLI integration (tags, link suggestions), ghost-text Tab-completion, MCP server exposing the vault.

## Design

All spacing should use multiples of 4 and be coherent.
All UI strings (labels, placeholders, error messages) are English; note content keeps its own language (`design/wireframes-v0.md` § Part I).
**No colour literal appears outside `assets/theme.css`** — every colour is a custom property, and both themes (dark `:root`, light `:root[data-theme="light"]`) are filled in together (`adr/2026-07-design-language-own-phase.md`).
On any conflict between `plan.md` and the wireframes, **the wireframes win** (`adr/2026-07-plan-realigned-with-wireframes.md`).

## Other instructions

- When writing comments, don't prefix them with `ponytail: `
- Don't hesitate to delegate to a cheaper model when it makes sense
- Never use while loops
- Code should be structured to avoid expect in the production code as much as possible
- Running `make test` should give 100% coverage once a feature is done implementing
