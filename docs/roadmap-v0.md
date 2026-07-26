# v0 Roadmap — daily driver for writing

Task breakdown of `plan.md` § Roadmap, v0.
The plan carries the *what* and *why*; this file carries the *order* and the *state*.
Ordering rationale: `adr/2026-07-v0-walking-skeleton-order.md`.
UI direction for every screen below: `design/wireframes-v0.md` (Part I) and `adr/2026-07-two-screens-table-and-logs.md` — the table is v1, the logs are v0, and there is no note list in the finished app.

**Next step → Phase 5, first unchecked box.**

## How we work

- Unless tagged otherwise: **Antoine writes the code, Claude writes the unit tests and guides.**
- Items marked **→ ADR** are decisions to take together at the start of the task, then record in `docs/adr/`.
- A phase is done when its exit criterion holds and `make test` passes with 100% coverage (phases 2+); a shortfall on regions is diagnosed per instantiation before writing any test (`adr/2026-07-coverage-100-percent-lines.md`).
- Per-task loop: discuss approach → decisions become ADRs → Claude guides the implementation → Claude writes tests → Antoine implements until green → commit.

## Phase 0 — Scaffolding

Goal: an empty Dioxus window opens and `make test` runs.

- [x] Read `.claude/dioxus.md` before any Dioxus code (both of us, every time).
- [x] Init cargo project, add Dioxus 0.7 desktop, render an empty window.
- [x] `makefile` with `make test` = tests + coverage report.
  - [x] Pick the coverage tool (suggestion: `cargo-llvm-cov`). **→ ADR** (`adr/2026-07-coverage-cargo-llvm-cov.md`)
- [x] Extend `.gitignore` (`target/`, coverage output).
- [x] Decide where the dev/test vault lives: fixture vault in `tests/`, real vault path outside the repo. **→ ADR** (`adr/2026-07-dev-test-vault-locations.md`)

Exit: `cargo run` opens a window; `make test` is green (even with zero tests).

## Phase 1 — Vault on disk (pure typst, no Rust)

Goal: a hand-made vault where every note compiles with the vanilla `typst` CLI.

- [x] Create the directory skeleton from `plan.md` § Storage layout (`.index/` gitignored).
- [x] Write `template.typ`: `#meta(id, type, created, tags, origin)`, `#l(id)`, base styles.
  - [x] Decide how notes import the template (relative `#import`, `--root`, package?). **→ ADR** (`adr/2026-07-template-import-from-root.md`)
- [x] Write per-type templates under `templates/`: `daily`, `capture`, and the permanent types (they can start near-identical).
- [x] Write sample notes in each category, linked to each other, including one deliberately dangling `#l` (test fodder for phases 3 and 8).
- [x] Add a `make check-vault` target that typst-compiles every `.typ` in the vault.

Exit: `make check-vault` compiles every note standalone.

## Phase 2 — Domain types + parsing (Rust, no UI)

Goal: given a `.typ` file, extract metadata and links; malformed input is data, not a crash.

- [x] Define domain types: `NoteId`, `Category`, `NoteType`, `Meta`, `Note`, `Link`.
  - [x] Missing/malformed `#meta` is an explicit variant (e.g. `MetaStatus`), never an error — the open-loops panel needs it as data. (`adr/2026-07-meta-anomalies-fine-grained-data.md`, `adr/2026-07-dates-jiff.md`)
- [x] Parse with `typst-syntax` (never regex):
  - [x] locate the `#meta(..)` call and extract its fields;
  - [x] extract all `#l("...")` calls and their targets.
- [x] *(Claude)* Unit tests: happy path, no meta, malformed meta args, unknown type, duplicate `#meta`, `#l` inside strings/comments, empty file.

Exit: parsing the phase-1 sample vault yields the expected notes and links; `make test` 100%.

## Phase 3 — Index (SQLite)

Goal: a queryable index rebuilt from files and kept live by a watcher.

- [x] Design the schema: notes, links, tags. **→ ADR** (`adr/2026-07-index-rusqlite.md`, `adr/2026-07-sqlite-index-schema.md`, `adr/2026-07-disposable-index-user-version.md`)
- [x] Full rebuild: walk the vault, parse (phase 2), populate `.index/`.
- [x] Queries: list notes (by category/type/tag), backlinks of a note, dangling links, typeless notes.
- [x] File watcher (`notify` crate): update the index on create/modify/delete. **→ ADR** (`adr/2026-07-incremental-vault-watching.md`)
  - [x] Debounce bursts; on anything ambiguous, fall back to a full rebuild — it must always be cheap enough for that.
- [x] *(Claude)* Tests: rebuild is idempotent, dangling/typeless detection, watcher integration test on a temp vault.

Exit: delete `.index/`, relaunch, everything is back — the invariant "index is always rebuildable" is now enforced by a test.

## Phase 4 — Walking skeleton: read-only app

Goal: open the app, see the note list, click a note, read it rendered.

- [x] Resolve the vault path at launch. **→ ADR** (`adr/2026-07-vault-path-at-launch.md`)
- [x] Embed the typst compiler crate: implement its `World` trait (fonts, file resolution rooted at the vault). *This is the hidden iceberg of the phase — budget accordingly.* **→ ADR** (`adr/2026-07-embedded-typst-world.md`)
- [x] Compile note → SVG with an in-memory cache keyed by content hash; invalidate on change. **→ ADR** (`adr/2026-07-svg-cache-per-path.md`)
- [x] Dioxus shell: note list from the index, click → rendered SVG view. **→ ADR** (`adr/2026-07-ui-covered-at-100.md`)
  - The list is deliberate scaffolding — the finished app has no list. It exists to prove the typst `World` works, and is deleted in phase 6; keep it dumb, don't grow features on it.
- [x] *(Claude)* Tests: compile a sample note to SVG, cache hit/invalidation logic, UI wiring via `VirtualDom` + `dioxus_ssr`.

Exit: you can browse and read the real vault in the app.

## Phase 5 — Writing: split-view editor, CRUD, daily notes

Goal: the app replaces Obsidian for daily notes — **start daily-driving at the end of this phase**.

- [ ] Buffer layer: text buffer + edit operations, strictly separate from the widget (the v2 vim invariant starts here).
  - [ ] Pick the buffer representation (suggestion: plain `String` — rope is YAGNI at note scale). **→ ADR**
- [ ] Split-view editor: source pane | rendered pane, debounced recompile.
- [ ] Saving: pick explicit save vs autosave. **→ ADR**
- [ ] Create note from template: pick type → instantiate template with `id`/`created` filled → open in editor.
  - [x] Id + filename scheme. **→ ADR** (`adr/2026-07-id-scheme-kebab-frozen.md`, decided during phase 1)
- [ ] Daily note: a "today" action creates today's note from the daily template if missing and wires prev/next links.
- [ ] Weekly + season notes (pulled into v0 by the logs screen, `adr/2026-07-two-screens-table-and-logs.md`):
  - [ ] `weekly.typ` and `seasonal.typ` templates, plus a season note in the fixture vault (only `daily.typ` and a `2026-w30` fixture exist today).
  - [ ] Define what a season *is* — boundaries and id form (`2026-été`: accented, so check it against `adr/2026-07-id-scheme-kebab-frozen.md`). **→ ADR**
  - [ ] Scale chain from a date: day → ISO week → season, resolved by `jiff` date math plus an index existence check, not by stored links.
- [ ] Delete note (dangling links it causes become visible through the index).
- [ ] *(Claude)* Tests: buffer ops, template instantiation, daily prev/next resolution across gaps, scale-chain resolution at year and season boundaries.

Exit: a full day's workflow (open today, write, link, save) happens in the app, not Obsidian.

## Phase 6 — The logs screen

Goal: day, week and season on one screen — the first screen from the design, and the one that kills the phase-4 list.

Spec: `design/wireframes-v0.md` § The logs screen (wireframe states `6c`–`6e`). Three panes, no tabs.

- [ ] Left: the time rail — one list where indentation carries the scale (season ⊃ week ⊃ day).
- [ ] Centre: the selected note rendered, with its clickable scale chain above it (phase-5 date math).
- [ ] Right: the jump panel — a compact month calendar marking days that have a note, plus week and season chips.
  - [ ] Index query: time notes within a date range (`notes_by_type` exists; it needs a date bound).
- [ ] Selecting in the rail or calendar swaps the centre pane; clicking an empty day *offers* creation from the template — navigating never writes a file.
- [ ] Delete the phase-4 scaffolding list.
- [ ] Decide whether the rail scrolls continuously or pages by month (design leaves it open). **→ ADR**
- [ ] *(Claude)* Tests: range query, rail ordering across scales, "note exists" marking, empty-day creation offered but not taken.

Exit: you navigate a month of logs without touching the filesystem, and the app has no list left in it.

## Phase 7 — Hybrid block editor

Goal: the block under the cursor shows source; every other block shows rendered output.

- [ ] Block segmentation from the `typst-syntax` tree (top-level markup nodes, never line-based).
- [ ] Map cursor position → active block; render inactive blocks as cached SVG fragments.
- [ ] Recompute block boundaries when the cursor leaves a block or on idle.
- [ ] Keep split view as a toggle.
- [ ] *(Claude)* Tests: segmentation on multi-line constructs, cursor→block mapping at boundaries.

Exit: hybrid editing feels better than split view for daily writing.
Pre-declared fallback (`plan.md` § Known risks): if this stalls, v0 ships on split view and this phase moves after v1.

## Phase 8 — Links UX

Goal: links are cheap to write and visible in both directions.

- [ ] `#l` insertion: hotkey + autocomplete over note ids/titles from the index.
- [ ] Backlinks panel on the open note.
- [ ] Dangling links visibly marked (in the panel at minimum).
- [ ] *(Claude)* Tests: autocomplete filtering, backlink query wiring.

Exit: you never type a full `#l("...")` by hand.

## Phase 9 — Capture + open-loops

Goal: zero-friction capture in, visible debt out — v0 complete.

- [ ] In-app capture: hotkey + paste → new file in `capture/` from the capture template, no required fields.
- [ ] Global (OS-level) capture strategy on Linux/Wayland: true global hotkey vs a DE shortcut launching `app --capture` through single-instance IPC. **→ ADR**
- [ ] Define what marks a capture "summarized" (suggestion: a non-empty summary block from the capture template). **→ ADR**
- [ ] Open-loops counter in the top bar of both screens; clicking it opens a flat list of unsummarized captures, dangling links and typeless notes. No ages, no grouping, no per-item actions — the queries already exist from phase 3 (`adr/2026-07-debt-counter-then-list.md`).
- [ ] *(Claude)* Tests: capture creation, summarized-detection, counter total matches the list.

Exit: v0 checklist below is fully green.

## v0 exit criteria (from `plan.md`)

- [ ] Vault structure + `#meta`/`#l` conventions + shared template (phase 1)
- [ ] File CRUD, creation from per-type templates, time notes (day/week/season) with prev/next (phase 5)
- [ ] The logs screen: time rail, rendered centre pane with scale chain, month calendar (phase 6)
- [ ] Hybrid block editor with live rendering — or split view via the declared fallback (phases 5, 7)
- [ ] Link index, backlinks panel, dangling-link detection (phases 3, 8)
- [ ] Capture notes + open-loops counter and list (phase 9)
- [ ] `make test` green with 100% coverage; every note compiles with vanilla typst
