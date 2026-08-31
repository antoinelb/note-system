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
- **Rendering**: the typst compiler embedded as a Rust crate — compile note → SVG, cache per note, invalidate on edit. The editor's view splits at the cursor: everything above the active line compiles as one Typst fragment, everything below as a second, and the active line alone stays the raw `<textarea>` — two region-level fragments, not one per line (`adr/2026-08-cursor-split-rendering.md`).
- **Index**: SQLite under `vault/.index/` (links, tags, positions, suggestions), rebuilt by parsing files, kept live by a file watcher.
- **Parsing**: extracting `#meta` and `#l` calls uses the `typst-syntax` crate — a real parse, never regex.

## Load-bearing invariants (constraints, not preferences)

- **Plain files are the source of truth.** The index is derived and must always be rebuildable from the `.typ` files; every note compiles standalone via the shared `template.typ`.
- **AI never writes prose in note files** (sole exception: the explicit `generated` type). Suggestions live only in the sidecar index; accepting a link suggestion = the user writes it (Tab-inserted ghost text), never an accept button that edits the file. Tab accepts the ghost text while one is showing and indents the line otherwise (`adr/2026-08-tab-indents-in-every-mode.md`).
- **No hard blocks.** All friction (unsummarized captures, unresolved suggestions, dangling links, typeless notes) is visible debt in the open-loops panel, never a save-blocker.
- **Canvas positions are user data disguised as index data** — they must survive index rebuilds.
- **Strict buffer/widget separation in the editor**, so the v2 vim modal layer can be inserted without a rewrite. Insert mode is the phase-0 writing flow: the grammar owns only Escape and Tab there, and what closes a pair or continues a list marker as you type belongs to the editor, not the grammar (`adr/2026-08-autopairs-in-the-typing-path.md`, `adr/2026-08-tab-indents-in-every-mode.md`).
- Note **type is a `#meta` field, not a directory**: directories encode only the four categories (`permanent/`, `time/`, `capture/`, `generated/`); the index, not the filesystem, is the authority for querying by type.
- **A block is still one physical line**, unchanged by the cursor-split rendering above: `blocks::segment` names the block map, and `dd`, `ip`/`ap`, and every motion still act on one line at a time. Only which blocks a compile groups together for *rendering* changed — never what a block *is* (`adr/2026-08-cursor-split-rendering.md`, `adr/2026-08-per-line-block-segmentation.md`).
- **The one register is the clipboard, and a verb that rewrites in place never touches it**: `gu`/`gU`/`g~` and visual `p` both diverge from vim here, because the register persists across applications and clobbering it costs more than vim's consistency buys (`adr/2026-08-case-operators-are-verbs.md`, `adr/2026-08-visual-gains-p-r-s-and-gv.md`).
- **The scroll anchor is consumed by the mount that uses it.** `zz`/`zt`/`zb` and every `j`/`k` name where the caret sits in the pane, and the next mount falls back to `Nearest` — the caret also remounts when an async compile lands, and a latched anchor would move the viewport with no keystroke behind it (`adr/2026-08-scroll-anchor-is-consumed-once.md`).
- **The ex line is literal and always global**: `:s/old/new/` replaces every occurrence in range with no regex and no `g` flag, and a line it cannot read speaks through the status surface rather than falling silent (`adr/2026-08-ex-line-is-literal-and-global.md`).
- **Deleting a note is unconfirmed and trashless**, from the palette's "delete note" row over an open sheet or immediately via Ctrl+Shift+D (guarded to a sheet already being open) — no confirmation dialog either path (`adr/2026-07-delete-unconfirmed-no-trash.md`, `adr/2026-08-delete-note-chord.md`).

## Roadmap

Four versions:
- **v0 — daily driver for writing**: vault structure + `#meta`/`#l` conventions, file CRUD from per-type templates, daily notes, hybrid block editor (fallback: a single pane toggling source ⇄ rendered), the design language (palette + type scale as theme variables, dark and light), the logs screen, link index + backlinks + dangling-link detection, capture notes + open-loops panel.
- **v1 — the table**: canvas with persistent positions, semantic zoom, modal card editing, filters, auto-placement. The v1 list is the ceiling, not the floor.
- **v2 — vim**: modal editing layer on the existing buffer architecture. The ceiling named six things it left out; the ex line has since crossed it, and marks, macros, named registers, visual block and the jumplist stay out with their reasons recorded (`adr/2026-08-the-ex-line-enters-the-v2-ceiling.md`).
- **v3 — AI**: `claude` CLI integration (tags, link suggestions), ghost-text Tab-completion, MCP server exposing the vault.

## Design

All spacing should use multiples of 4 and be coherent — that is UI pixels; one indentation level in a note's *text* is two spaces, `caret::INDENT`, Typst's own nesting width (`adr/2026-08-tab-indents-in-every-mode.md`).

All UI strings (labels, placeholders, error messages) are English; note content keeps its own language (`adr/2026-07-repo-language-english.md`).
**No colour literal appears outside `assets/theme.css`** — every colour is a custom property, and both themes (dark `:root`, light `:root[data-theme="light"]`) are filled in together (`adr/2026-07-design-language-own-phase.md`).

One `--prose-size` token drives both the editor's textarea and the rendered SVG's type scale — no separate size for source and render (`adr/2026-08-one-font-size-for-source-and-render.md`). A settings overlay (Ctrl+,) holds the only controls that move it, alongside a theme toggle; both are session-only, with no persistence file (`adr/2026-08-settings-overlay.md`).

Every aspect of the interface should be keyboard-driven first and then usable by mouse.

## Other instructions

- Implementation is Claude's job — code, tests, docs; the user directs, decides, and reviews (`adr/2026-08-implementation-is-claudes-job.md`)
- When writing comments, don't prefix them with `ponytail: `
- Don't hesitate to delegate to a cheaper model when it makes sense
- Never use while loops
- Code should be structured to avoid expect in the production code as much as possible
- Running `make test` should give 100% coverage once a feature is done implementing
- `make test` sees markup, never the window: `make e2e` drives the shipped binary with real keystrokes in a headless X server and asserts on the `.typ` files they produce (`adr/2026-08-headless-x11-e2e.md`). It is not in the pre-commit hook. `tests/e2e/session.sh` opens the same app for the `note-taker` persona to explore by hand; its report is never committed — a finding becomes an ADR or a scenario
- Always delegate commit to a haiku agent
