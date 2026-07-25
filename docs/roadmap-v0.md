# v0 Roadmap — daily driver for writing

Task breakdown of `plan.md` § Roadmap, v0.
The plan carries the *what* and *why*; this file carries the *order* and the *state*.
Ordering rationale: `adr/2026-07-v0-walking-skeleton-order.md`.

**Next step → Phase 4, first unchecked box.**

## How we work

- Unless tagged otherwise: **Antoine writes the code, Claude writes the unit tests and guides.**
- Items marked **→ ADR** are decisions to take together at the start of the task, then record in `docs/adr/`.
- A phase is done when its exit criterion holds and `make test` passes with 100% coverage (phases 2+); a shortfall on regions is diagnosed per instantiation before writing any test (`adr/2026-07-couverture-100-pourcent-lignes.md`).
- Per-task loop: discuss approach → decisions become ADRs → Claude guides the implementation → Claude writes tests → Antoine implements until green → commit.

## Phase 0 — Scaffolding

Goal: an empty Dioxus window opens and `make test` runs.

- [x] Read `.claude/dioxus.md` before any Dioxus code (both of us, every time).
- [x] Init cargo project, add Dioxus 0.7 desktop, render an empty window.
- [x] `makefile` with `make test` = tests + coverage report.
  - [x] Pick the coverage tool (suggestion: `cargo-llvm-cov`). **→ ADR** (`adr/2026-07-couverture-cargo-llvm-cov.md`)
- [x] Extend `.gitignore` (`target/`, coverage output).
- [x] Decide where the dev/test vault lives: fixture vault in `tests/`, real vault path outside the repo. **→ ADR** (`adr/2026-07-emplacement-vaults-dev-test.md`)

Exit: `cargo run` opens a window; `make test` is green (even with zero tests).

## Phase 1 — Vault on disk (pure typst, no Rust)

Goal: a hand-made vault where every note compiles with the vanilla `typst` CLI.

- [x] Create the directory skeleton from `plan.md` § Storage layout (`.index/` gitignored).
- [x] Write `template.typ`: `#meta(id, type, created, tags, origin)`, `#l(id)`, base styles.
  - [x] Decide how notes import the template (relative `#import`, `--root`, package?). **→ ADR** (`adr/2026-07-import-template-racine.md`)
- [x] Write per-type templates under `templates/`: `daily`, `capture`, and the permanent types (they can start near-identical).
- [x] Write sample notes in each category, linked to each other, including one deliberately dangling `#l` (test fodder for phases 3 and 7).
- [x] Add a `make check-vault` target that typst-compiles every `.typ` in the vault.

Exit: `make check-vault` compiles every note standalone.

## Phase 2 — Domain types + parsing (Rust, no UI)

Goal: given a `.typ` file, extract metadata and links; malformed input is data, not a crash.

- [x] Define domain types: `NoteId`, `Category`, `NoteType`, `Meta`, `Note`, `Link`.
  - [x] Missing/malformed `#meta` is an explicit variant (e.g. `MetaStatus`), never an error — the open-loops panel needs it as data. (`adr/2026-07-anomalies-meta-donnees-fines.md`, `adr/2026-07-dates-jiff.md`)
- [x] Parse with `typst-syntax` (never regex):
  - [x] locate the `#meta(..)` call and extract its fields;
  - [x] extract all `#l("...")` calls and their targets.
- [x] *(Claude)* Unit tests: happy path, no meta, malformed meta args, unknown type, duplicate `#meta`, `#l` inside strings/comments, empty file.

Exit: parsing the phase-1 sample vault yields the expected notes and links; `make test` 100%.

## Phase 3 — Index (SQLite)

Goal: a queryable index rebuilt from files and kept live by a watcher.

- [x] Design the schema: notes, links, tags. **→ ADR** (`adr/2026-07-index-rusqlite.md`, `adr/2026-07-schema-index-sqlite.md`, `adr/2026-07-index-jetable-user-version.md`)
- [x] Full rebuild: walk the vault, parse (phase 2), populate `.index/`.
- [x] Queries: list notes (by category/type/tag), backlinks of a note, dangling links, typeless notes.
- [x] File watcher (`notify` crate): update the index on create/modify/delete. **→ ADR** (`adr/2026-07-surveillance-incrementale-du-vault.md`)
  - [x] Debounce bursts; on anything ambiguous, fall back to a full rebuild — it must always be cheap enough for that.
- [x] *(Claude)* Tests: rebuild is idempotent, dangling/typeless detection, watcher integration test on a temp vault.

Exit: delete `.index/`, relaunch, everything is back — the invariant "index is always rebuildable" is now enforced by a test.

## Phase 4 — Walking skeleton: read-only app

Goal: open the app, see the note list, click a note, read it rendered.

- [ ] Embed the typst compiler crate: implement its `World` trait (fonts, file resolution rooted at the vault). *This is the hidden iceberg of the phase — budget accordingly.*
- [ ] Compile note → SVG with an in-memory cache keyed by content hash; invalidate on change.
- [ ] Dioxus shell: note list from the index, click → rendered SVG view.
- [ ] *(Claude)* Tests: compile a sample note to SVG, cache hit/invalidation logic.

Exit: you can browse and read the real vault in the app.

## Phase 5 — Writing: split-view editor, CRUD, daily notes

Goal: the app replaces Obsidian for daily notes — **start daily-driving at the end of this phase**.

- [ ] Buffer layer: text buffer + edit operations, strictly separate from the widget (the v2 vim invariant starts here).
  - [ ] Pick the buffer representation (suggestion: plain `String` — rope is YAGNI at note scale). **→ ADR**
- [ ] Split-view editor: source pane | rendered pane, debounced recompile.
- [ ] Saving: pick explicit save vs autosave. **→ ADR**
- [ ] Create note from template: pick type → instantiate template with `id`/`created` filled → open in editor.
  - [x] Id + filename scheme. **→ ADR** (`adr/2026-07-schema-id-kebab-fige.md`, decided during phase 1)
- [ ] Daily note: a "today" action creates today's note from the daily template if missing and wires prev/next links.
- [ ] Delete note (dangling links it causes become visible through the index).
- [ ] *(Claude)* Tests: buffer ops, template instantiation, daily prev/next resolution across gaps.

Exit: a full day's workflow (open today, write, link, save) happens in the app, not Obsidian.

## Phase 6 — Hybrid block editor

Goal: the block under the cursor shows source; every other block shows rendered output.

- [ ] Block segmentation from the `typst-syntax` tree (top-level markup nodes, never line-based).
- [ ] Map cursor position → active block; render inactive blocks as cached SVG fragments.
- [ ] Recompute block boundaries when the cursor leaves a block or on idle.
- [ ] Keep split view as a toggle.
- [ ] *(Claude)* Tests: segmentation on multi-line constructs, cursor→block mapping at boundaries.

Exit: hybrid editing feels better than split view for daily writing.
Pre-declared fallback (`plan.md` § Known risks): if this stalls, v0 ships on split view and this phase moves after v1.

## Phase 7 — Links UX

Goal: links are cheap to write and visible in both directions.

- [ ] `#l` insertion: hotkey + autocomplete over note ids/titles from the index.
- [ ] Backlinks panel on the open note.
- [ ] Dangling links visibly marked (in the panel at minimum).
- [ ] *(Claude)* Tests: autocomplete filtering, backlink query wiring.

Exit: you never type a full `#l("...")` by hand.

## Phase 8 — Capture + open-loops panel

Goal: zero-friction capture in, visible debt out — v0 complete.

- [ ] In-app capture: hotkey + paste → new file in `capture/` from the capture template, no required fields.
- [ ] Global (OS-level) capture strategy on Linux/Wayland: true global hotkey vs a DE shortcut launching `app --capture` through single-instance IPC. **→ ADR**
- [ ] Define what marks a capture "summarized" (suggestion: a non-empty summary block from the capture template). **→ ADR**
- [ ] Open-loops panel, always accessible: unsummarized captures (with age), dangling links, typeless notes.
- [ ] *(Claude)* Tests: capture creation, summarized-detection, panel queries.

Exit: v0 checklist below is fully green.

## v0 exit criteria (from `plan.md`)

- [ ] Vault structure + `#meta`/`#l` conventions + shared template (phase 1)
- [ ] File CRUD, creation from per-type templates, daily note with prev/next (phase 5)
- [ ] Hybrid block editor with live rendering — or split view via the declared fallback (phases 5–6)
- [ ] Link index, backlinks panel, dangling-link detection (phases 3, 7)
- [ ] Capture notes + open-loops panel (phase 8)
- [ ] `make test` green with 100% coverage; every note compiles with vanilla typst
