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
- **Rendering**: a parse-tree markup model (`src/markup.rs`) draws each block's prose as styled HTML spans laid out by the browser — no compile, no reflow, on a cursor move. The embedded typst compiler stays for whatever CSS cannot draw as one content-keyed widget per block (an equation, a table, a figure, any construct the markup model's node-kind verdict doesn't own), and for the table's card bodies, which still compile whole notes (`adr/2026-08-css-draws-the-markup.md`). A changed fallback block keeps its last SVG on screen, dimmed, until the recompile lands (`adr/2026-09-fragments-shelve-their-last-svg-per-block.md`). An inactive block hides its syntax — markers, delimiters, `[ ]` — with CSS alone, the DOM still tiling the source; the line under the caret and a selected line show the source (`adr/2026-09-inactive-blocks-hide-their-syntax.md`).
- **Index**: SQLite under `vault/.index/` (links, tags, positions, suggestions, and an FTS5 table over each note's text past its preamble, searched by Ctrl+Shift+F with a literal word query — `adr/2026-09-full-text-search-lives-in-the-index.md`), rebuilt by parsing files, kept live two ways: a file watcher covers changes from outside the app, and five write seams (Ctrl+N, Ctrl+D and the relative-time commands, Ctrl+Shift+V capture, delete, and undo-of-delete) index their own write in the same tick rather than waiting on the watcher's round trip (`adr/2026-09-the-app-indexes-its-own-writes.md`).
- **Parsing**: extracting `#meta` and `#l` calls uses the `typst-syntax` crate — a real parse, never regex.

## Load-bearing invariants (constraints, not preferences)

- **Plain files are the source of truth.** The index is derived and must always be rebuildable from the `.typ` files; every note compiles standalone via the shared `template.typ`.
- **AI never writes prose in note files** (sole exception: the explicit `generated` type, whose `generated/` directory is the one place machine prose lives, has no template and no Ctrl+N row, and is listed on the rail like a capture — `adr/2026-09-generated-is-the-one-place-ai-prose-lives.md`). Suggestions live only in the sidecar index; accepting a link suggestion = the user writes it (Tab-inserted ghost text), never an accept button that edits the file. Tab accepts the ghost text while one is showing and indents the line otherwise (`adr/2026-08-tab-indents-in-every-mode.md`).
- **No hard blocks.** All friction (unsummarized captures, unresolved suggestions, dangling links, typeless notes) is visible debt in the open-loops panel, never a save-blocker.
- **Canvas positions are user data disguised as index data** — they must survive index rebuilds.
- **Strict buffer/widget separation in the editor**, so the v2 vim modal layer can be inserted without a rewrite. Insert mode is the phase-0 writing flow: the grammar owns only Escape and Tab there, and what closes a pair or continues a list marker as you type belongs to the editor, not the grammar (`adr/2026-08-autopairs-in-the-typing-path.md`, `adr/2026-08-tab-indents-in-every-mode.md`).
- Note **type is a `#meta` field, not a directory**: directories encode only the four categories (`permanent/`, `time/`, `capture/`, `generated/`); the index, not the filesystem, is the authority for querying by type.
- **A paste with no text on the clipboard pastes its image**: `p`, `P` and insert-mode Ctrl+V write the PNG to `vault/assets/<stem>-<stamp>.png`, the first non-`.typ` files in the vault, and spell `#image("/assets/…")`; the watcher and the index never see it (`adr/2026-09-an-image-pastes-into-assets.md`).
- **`#l` links notes; Typst's own `#link(dest)[body]` links what is not a note** — a PDF under `vault/assets/`, a URL — and following one hands the destination to the desktop's opener (`xdg-open`, started and never awaited); a leading `/` is the vault root. The index never counts a `#link` as debt (`adr/2026-09-link-is-for-resources.md`).
- **Eight permanent types** — a course is a `project`, a lecture a `source` linking it, and the knowledge that outlives the course lives in the permanent notes it links (`adr/2026-09-a-course-is-a-project.md`) — and **`due: "YYYY-MM-DD"` is a `#meta` field any note may carry**: the open-loops list names a note overdue or due within seven days, last in the list, one date spoken where the date is the debt — the one amendment to the loops' "no ages" vocabulary (`adr/2026-09-course-type-and-due-loops.md`).
- **A block is still one physical line**, unchanged by which renderer draws it: `blocks::segment` names the block map, and `dd`, `ip`/`ap`, and every motion still act on one line at a time. Only what draws a block's content changed — never what a block *is* (`adr/2026-08-per-line-block-segmentation.md`, `adr/2026-08-css-draws-the-markup.md`).
- **The editor draws with CSS; export and the table's card bodies compile with Typst.** The palette's "export pdf" flushes the open note, compiles it with the paper theme on the compute tier and writes `<stem>.pdf` beside it, a receipt or the refused stage on the status line (`adr/2026-09-export-writes-the-pdf-beside-the-note.md`). A block's own parse-tree node-kind verdict decides, per block, which one draws it — CSS for markup the model owns, the embedded compiler otherwise — so the two pipelines can diverge in mechanism as long as they agree in appearance (`adr/2026-08-css-draws-the-markup.md`).
- **The one register is the clipboard, and a verb that rewrites in place never touches it**: `gu`/`gU`/`g~` and visual `p` both diverge from vim here, because the register persists across applications and clobbering it costs more than vim's consistency buys (`adr/2026-08-case-operators-are-verbs.md`, `adr/2026-08-visual-gains-p-r-s-and-gv.md`).
- **The scroll anchor is consumed by the mount that uses it.** `zz`/`zt`/`zb` and every `j`/`k` name where the caret sits in the pane, and the next mount falls back to `Nearest` — the caret also remounts when an async compile lands, and a latched anchor would move the viewport with no keystroke behind it (`adr/2026-08-scroll-anchor-is-consumed-once.md`).
- **The ex line is literal and always global**: `:s/old/new/` replaces every occurrence in range with no regex and no `g` flag, and a line it cannot read speaks through the status surface rather than falling silent (`adr/2026-08-ex-line-is-literal-and-global.md`).
- **Deleting a note is unconfirmed and trashless**, from the palette's "delete note" row over an open sheet or immediately via Ctrl+Shift+D (guarded to a sheet already being open) — no confirmation dialog either path (`adr/2026-07-delete-unconfirmed-no-trash.md`, `adr/2026-08-delete-note-chord.md`). The deep recovery is git: the real vault is a repository committed by a systemd user timer every five minutes, and the app knows nothing about it (`adr/2026-09-git-backs-up-the-vault-outside-the-app.md`).
- **A key that beats an overlay's focus grab belongs to the overlay.** Every overlay grabs focus in an async `onmounted`; a bare key that reaches the sink or a pane while one is up is relayed into its query (or dropped when it has none) and never reaches the grammar — Escape and the chords still pass. Enter is dropped, not forwarded: the known ceiling (`adr/2026-09-overlay-keys-relay-before-focus-lands.md`). The field's own `input` event is read as a delta against what the field may still be showing (the last value it reported and every write since), never taken as the whole query, because the relayed letters reach the field by a patch that races the focus grab (`adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md`).
- **A loop line opens the note that owes the debt.** Arrows highlight a rank (clamped, no wrap), Enter or a row's own click opens it by path — never by id, since some of what the loops list names (a note with no `#meta` at all) has no id row for the index to resolve — and the container's own click or Escape still just closes the overlay (`adr/2026-09-loop-lines-open-their-notes.md`, superseding `adr/2026-08-loops-list-overlay.md`'s "nothing clickable" clause). The destination follows the category rule: a `time/` note lands on the logs (its file opens even when the rail excludes it as debt; only a stem no scale can parse falls back to the sheet), everything else opens the sheet. Following a dangling link (`gf`, Ctrl+Enter, the palette) is unrelated and unchanged: the target still stays inert (`adr/2026-08-permanent-links-open-sheets.md`).
- **An index-read notice resolves on the next successful index read** for the same source, gated the same way every other notice source is, plus the one case a read alone can't prove: leaving the closed editor a failed lookup produced also resolves it (`adr/2026-09-index-notices-resolve-on-a-good-lookup.md`).
- **There is no log file and no logging crate.** The status surface is the windowed app's log; stderr belongs to the headless `--capture` process alone, and the e2e harness's `app.log` is the harness's file, not the app's (`adr/2026-09-the-status-surface-is-the-only-log.md`).

## Roadmap

Four versions; v0, v1 and v2 are shipped and v3 has no code yet, its six open decisions listed in `adr/2026-08-v3-mcp-first-order.md`. The Cargo version names the install, `0.N.0` for roadmap version vN in daily use, so `0.2.0` is v2 and `0.3.0` will be the first install carrying v3 (`adr/2026-09-the-cargo-version-names-the-install.md`).
- **v0 — daily driver for writing**: vault structure + `#meta`/`#l` conventions, file CRUD from per-type templates, daily notes, hybrid block editor (fallback: a single pane toggling source ⇄ rendered), the design language (palette + type scale as theme variables, dark and light), the logs screen, link index + backlinks + dangling-link detection, capture notes + open-loops panel.
- **v1 — the table**: canvas with persistent positions, semantic zoom, modal card editing, filters, auto-placement. The v1 list is the ceiling, not the floor.
- **v2 — vim**: modal editing layer on the existing buffer architecture. The ceiling named seven things it left out; the ex line has since crossed it, and marks, macros, named registers, visual block, the jumplist and configuration stay out with their reasons recorded (`adr/2026-08-the-ex-line-enters-the-v2-ceiling.md`).
- **v3 — AI**: `claude` CLI integration (tags, link suggestions), ghost-text Tab-completion, MCP server exposing the vault.

## Design

All spacing should use multiples of 4 and be coherent — that is UI pixels; one indentation level in a note's *text* is two spaces, `caret::INDENT`, Typst's own nesting width (`adr/2026-08-tab-indents-in-every-mode.md`).

All UI strings (labels, placeholders, error messages) are English; note content keeps its own language (`adr/2026-07-repo-language-english.md`).
A line beginning `> ` is a block quote, stored literally and taught to vanilla Typst by a `show par:` rule in `templates/template.typ` — no editor-side rewrite (`adr/2026-08-greater-than-is-the-stored-quote.md`).
**No colour literal appears outside `assets/theme.css`** — every colour is a custom property, and both themes (dark `:root`, light `:root[data-theme="light"]`) are filled in together (`adr/2026-07-design-language-own-phase.md`).

One `--prose-size` token drives both the editor's textarea and the rendered SVG's type scale — no separate size for source and render (`adr/2026-08-one-font-size-for-source-and-render.md`). A settings overlay (Ctrl+,) holds the only controls that move it, alongside a theme toggle; both are session-only, with no persistence file (`adr/2026-08-settings-overlay.md`).
On the logs, Alt+H folds the rail and Alt+L the jump panel — normal mode, the pane itself, or the palette rows; a folded pane keeps its hairline, insert mode keeps every AltGr character, and the folds are session-only too (`adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md`).

Every aspect of the interface should be keyboard-driven first and then usable by mouse.

## Other instructions

- Implementation is Claude's job — code, tests, docs; the user directs, decides, and reviews (`adr/2026-08-implementation-is-claudes-job.md`)
- When writing comments, don't prefix them with `ponytail: `
- Don't hesitate to delegate to a cheaper model when it makes sense
- Never use while loops
- Code should be structured to avoid expect in the production code as much as possible
- Running `make test` should give 100% coverage once a feature is done implementing. The gate excuses `lib.rs`, `mod.rs` and the one function `fn main`, nothing else: what `main` hands the window lives in `src/launch.rs` and is covered (`adr/2026-09-main-holds-only-the-launch-builder.md`). `tests/integration/properties.rs` holds three `proptest` properties — any key sequence leaves the editor sound and undo walks back, CSS spans tile a block's source, the parser survives anything (`adr/2026-09-property-tests-guard-three-invariants.md`)
- `make test` sees markup, never the window: `make e2e` drives the shipped binary with real keystrokes in a headless X server and asserts on the `.typ` files and the index they produce — never on pixels (`adr/2026-08-headless-x11-e2e.md`). The app's clock is pinned to `E2E_TODAY` through `NOTE_TODAY` (`adr/2026-09-note-today-pins-the-clock-for-e2e.md`). The pre-commit hook is the CI and runs the other three gates; `make upgrade` runs `make e2e` before installing (`adr/2026-09-the-pre-commit-hook-is-the-ci.md`). `tests/e2e/session.sh` opens the same app for the `note-taker` persona to explore by hand after a batch of interface work lands; its report is never committed — a finding becomes a plan under `docs/plans/` with a scenario per fix, an ADR, or a named out-of-scope line (`adr/2026-09-a-note-taker-report-becomes-an-adr-or-a-scenario.md`)
- Always delegate commit to a haiku agent
