# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A personal Typst knowledge system: a Linux desktop app built with Dioxus, storing every note as a plain `.typ` file compilable by the vanilla typst CLI.
It replaces Obsidian (daily notes) and a manual typst zettelkasten.
Single user, no accounts, no plugins — extended by editing the source.

## Documents

- `docs/adr/` — decision records, **the only documentation**: the plan and wireframe docs are gone; the what lives in this file and the code, the why lives in the ADRs.

## Decision records (ADR) — required practice

Every decision taken from now on is documented in its own file under `docs/adr/`:

- One decision per file, kebab-case name (e.g. `2026-07-positions-separate-file.md`).
- Keep it short: context, the decision, alternatives rejected and why.
- When a decision changes something stated in this file, update CLAUDE.md *and* add the ADR — CLAUDE.md carries the **what**, the ADR preserves the **why**.
- When you (Claude) participate in a decision with the user, write the ADR as part of the same change; do not let decisions live only in conversation.
- When committing there's a decision that's unclear, ask the user why something was made the way it was to document it in an ADR.

## Stack and architecture (decided)

Rust throughout — reasoning preserved in the ADRs:

- **UI**: Dioxus 0.7 desktop. **Whenever Dioxus code is written or understood, first read `.claude/dioxus.md`** (Dioxus 0.7 API reference): 0.7 changed every API — `cx`, `Scope`, and `use_state` are gone; use `use_signal`, `#[component]`, `rsx!`, `Routable`, `use_resource`.
- **Rendering**: a parse-tree markup model (`src/markup.rs`) draws each block's prose as styled HTML spans laid out by the browser — no compile, no reflow, on a cursor move. The embedded typst compiler stays for whatever CSS cannot draw as one content-keyed widget per block (an equation, a table, a figure, any construct the markup model's node-kind verdict doesn't own), and for the table's card bodies, which still compile whole notes (`adr/2026-08-css-draws-the-markup.md`).
- **Index**: SQLite under `vault/.index/` (links, tags, positions, suggestions), rebuilt by parsing files, kept live by a file watcher.
- **Parsing**: extracting `#meta` and `#l` calls uses the `typst-syntax` crate — a real parse, never regex.

## Load-bearing invariants (constraints, not preferences)

- **Plain files are the source of truth.** The index is derived and must always be rebuildable from the `.typ` files; every note compiles standalone via the shared `template.typ`.
- **AI never writes prose in note files** (sole exception: the explicit `generated` type). Suggestions live only in the sidecar index; accepting a link suggestion = the user writes it (Tab-inserted ghost text), never an accept button that edits the file. Tab accepts the ghost text while one is showing and indents the line otherwise (`adr/2026-08-tab-indents-in-every-mode.md`).
- **No hard blocks.** All friction (unsummarized captures, unresolved suggestions, dangling links, typeless notes) is visible debt in the open-loops panel, never a save-blocker.
- **Canvas positions are user data disguised as index data** — they must survive index rebuilds.
- **Strict buffer/widget separation in the editor**, so the v2 vim modal layer can be inserted without a rewrite. Insert mode is the phase-0 writing flow: the grammar owns only Escape and Tab there, and what closes a pair or continues a list marker as you type belongs to the editor, not the grammar (`adr/2026-08-autopairs-in-the-typing-path.md`, `adr/2026-08-tab-indents-in-every-mode.md`).
- Note **type is a `#meta` field, not a directory**: directories encode only the four categories (`permanent/`, `time/`, `capture/`, `generated/`); the index, not the filesystem, is the authority for querying by type.
- **A block is still one physical line**, unchanged by which renderer draws it: `blocks::segment` names the block map, and `dd`, `ip`/`ap`, and every motion still act on one line at a time. Only what draws a block's content changed — never what a block *is* (`adr/2026-08-per-line-block-segmentation.md`, `adr/2026-08-css-draws-the-markup.md`).
- **The editor draws with CSS; export and the table's card bodies compile with Typst.** A block's own parse-tree node-kind verdict decides, per block, which one draws it — CSS for markup the model owns, the embedded compiler otherwise — so the two pipelines can diverge in mechanism as long as they agree in appearance (`adr/2026-08-css-draws-the-markup.md`).
- **Deleting a note is unconfirmed and trashless**, from the palette's "delete note" row over an open sheet or immediately via Ctrl+Shift+D (guarded to a sheet already being open) — no confirmation dialog either path (`adr/2026-07-delete-unconfirmed-no-trash.md`, `adr/2026-08-delete-note-chord.md`).

## Roadmap

Four versions:
- **v0 — daily driver for writing**: vault structure + `#meta`/`#l` conventions, file CRUD from per-type templates, daily notes, hybrid block editor (fallback: a single pane toggling source ⇄ rendered), the design language (palette + type scale as theme variables, dark and light), the logs screen, link index + backlinks + dangling-link detection, capture notes + open-loops panel.
- **v1 — the table**: canvas with persistent positions, semantic zoom, modal card editing, filters, auto-placement. The v1 list is the ceiling, not the floor.
- **v2 — vim**: modal editing layer on the existing buffer architecture.
- **v3 — AI**: `claude` CLI integration (tags, link suggestions), ghost-text Tab-completion, MCP server exposing the vault.

## Design

All spacing should use multiples of 4 and be coherent — that is UI pixels; one indentation level in a note's *text* is two spaces, `caret::INDENT`, Typst's own nesting width (`adr/2026-08-tab-indents-in-every-mode.md`).

All UI strings (labels, placeholders, error messages) are English; note content keeps its own language (`adr/2026-07-repo-language-english.md`).
A line beginning `> ` is a block quote, stored literally and taught to vanilla Typst by a `show par:` rule in `templates/template.typ` — no editor-side rewrite (`adr/2026-08-greater-than-is-the-stored-quote.md`).
**No colour literal appears outside `assets/theme.css`** — every colour is a custom property, and both themes (dark `:root`, light `:root[data-theme="light"]`) are filled in together (`adr/2026-07-design-language-own-phase.md`).

One `--prose-size` token drives both the editor's textarea and the rendered SVG's type scale — no separate size for source and render (`adr/2026-08-one-font-size-for-source-and-render.md`). A settings overlay (Ctrl+,) holds the only controls that move it, alongside a theme toggle; both are session-only, with no persistence file (`adr/2026-08-settings-overlay.md`).

## Other instructions

- Implementation is Claude's job — code, tests, docs; the user directs, decides, and reviews (`adr/2026-08-implementation-is-claudes-job.md`)
- When writing comments, don't prefix them with `ponytail: `
- Don't hesitate to delegate to a cheaper model when it makes sense
- Never use while loops
- Code should be structured to avoid expect in the production code as much as possible
- Running `make test` should give 100% coverage once a feature is done implementing
- Always delegate commit to a haiku agent
