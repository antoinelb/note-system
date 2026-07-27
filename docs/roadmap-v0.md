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
- [x] Write sample notes in each category, linked to each other, including one deliberately dangling `#l` (test fodder for phases 3 and 9).
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
  - The list is deliberate scaffolding — the finished app has no list. It exists to prove the typst `World` works, and is deleted in phase 7; keep it dumb, don't grow features on it.
- [x] *(Claude)* Tests: compile a sample note to SVG, cache hit/invalidation logic, UI wiring via `VirtualDom` + `dioxus_ssr`.

Exit: you can browse and read the real vault in the app.

## Phase 5 — Writing: split-view editor, CRUD, daily notes

Goal: the app replaces Obsidian for daily notes — **start daily-driving at the end of this phase**.

- [x] Buffer layer: text buffer + edit operations, strictly separate from the widget (the v2 vim invariant starts here).
  - [x] Pick the buffer representation (suggestion: plain `String` — rope is YAGNI at note scale). **→ ADR** (`adr/2026-07-buffer-is-path-plus-string.md`; edit operations deferred to phase 8, which is the first code that can call them)
- [x] Split-view editor: source pane | rendered pane, debounced recompile.
  - Like the phase-4 list, this is **deliberate scaffolding with a known deletion date**: the design has no split view anywhere, and neither the phase-7 logs centre pane nor v1's writing sheet has room for two panes. It exists so writing can start before the real screen does, and it is deleted in phase 7 — the buffer underneath survives, the two-pane widget does not. Keep it dumb.
- [x] Saving: pick explicit save vs autosave. **→ ADR** (`adr/2026-07-debounced-autosave.md` — one idle timer drives save then recompile)
- [ ] Create note from template: pick type → instantiate template with `id`/`created` filled → open in editor.
  - [x] Id + filename scheme. **→ ADR** (`adr/2026-07-id-scheme-kebab-frozen.md`, decided during phase 1; collision suffix superseded by `adr/2026-07-id-collision-is-an-error.md`)
  - [x] The `{{...}}` placeholder contract: which names exist, and what an unknown one does. **→ ADR** (`adr/2026-07-template-placeholders-closed-set.md`)
- [ ] Daily note: a "today" action creates today's note from the daily template if missing.
  - [ ] Previous/next movement resolves to the nearest *existing* daily note through the index; the template seeds no links (`adr/2026-07-time-navigation-derived-not-stored.md`).
- [ ] Weekly + season notes (pulled into v0 by the logs screen, `adr/2026-07-two-screens-table-and-logs.md`):
  - [ ] `weekly.typ` and `seasonal.typ` templates, plus a season note in the fixture vault (only `daily.typ` and a `2026-w30` fixture exist today).
  - [ ] Define what a season *is* — the **boundaries**; the id form is already fixed at `2026-summer` by the design's rail rows (`design/wireframes-v0.md` § The logs screen), which also retires the accented `2026-été` that `adr/2026-07-repo-language-english.md` had earmarked as id-scheme coverage. **→ ADR**
  - [ ] Scale chain from a date: day → ISO week → season, resolved by `jiff` date math plus an index existence check, not by stored links.
- [ ] Delete note (dangling links it causes become visible through the index).
- [ ] *(Claude)* Tests: template instantiation, unknown placeholders, refusing to overwrite an existing note, daily prev/next resolution across gaps, scale-chain resolution at year and season boundaries.

Exit: a full day's workflow (open today, write, link, save) happens in the app, not Obsidian.

## Phase 6 — The design language

Goal: the frozen design exists as code, so every screen after this one is built in it rather than restyled into it.

Spec: `design/wireframes-v0.md` § Chrome, § Palette, § Typography. Decision: `adr/2026-07-design-language-own-phase.md`.
Nothing here is a new screen — it is the vocabulary the logs screen is then written in.

- [ ] `assets/theme.css`: the "Deep field" palette as custom properties, **both themes from the first rule** — `:root` dark, `:root[data-theme="light"]` light. No colour literal appears outside this file, ever.
  - [ ] Decide how the theme is chosen: OS `prefers-color-scheme`, an explicit toggle, or both. **→ ADR**
- [ ] Type scale: `ui-sans-serif` chrome, `ui-monospace` 9–10px for all metadata/labels/rail/calendar (uppercase + `.09em` for type labels and the month header).
- [ ] The one-line chrome: two 14×14 stroked icons (table · logs) left, current lit and the other dim; the open-loops ember right. **Zero loops renders nothing at all** — absence, not a zero.
- [ ] Normalize the mockups' spacing to multiples of 4 (`design/wireframes-v0.md` § Implementation notes: 9/11/18 → 8/12/16/20), keeping proportions rather than pixel values.
- [ ] Note prose lives in `templates/template.typ`, not in the app: give it the design's serif body and title scale (`§ Typography`), since the app never restyles rendered note bodies.
- [ ] Wire the stylesheet in: `document::Stylesheet { href: asset!("/assets/theme.css") }`.
- [ ] *(Claude)* Tests: the ember is absent at zero and present at non-zero, the lit icon follows the current screen, the theme attribute switches the resolved variables.

Exit: both themes render the phase-4 shell in the real palette, and `grep` finds no colour literal outside `theme.css`.

## Phase 7 — The logs screen

Goal: day, week and season on one screen — the first screen from the design, and the one that kills the phase-4 list.

Spec: `design/wireframes-v0.md` § The logs screen (wireframe states `6c`–`6e`). Three panes, no tabs, in the phase-6 language.

- [ ] Left: the time rail — one list where indentation carries the scale (season ⊃ week ⊃ day); no boxes, no fills, the selected day is simply bold bright ink.
- [ ] Centre: the selected note rendered, with its clickable scale chain above it (phase-5 date math).
  - [ ] The **"captured today"** block under the day note: that day's captures and generated notes as plain mono lines ("capture-articles-zettel · still open") — the day gathers what happened in it.
- [ ] Right: the jump panel — a month grid where bright ink = a note exists and faint = empty, the week-number gutter is clickable as a scale, and a season row sits below. No chips, no ‹ › buttons: months page by scrolling.
  - [ ] Index query: time notes within a date range (`notes_by_type` exists; it needs a date bound). The same bound serves the "captured today" block.
- [ ] Selecting in the rail or calendar swaps the centre pane. A selected empty day is **outlined, not filled** (selection ≠ existence) and shows one centred line offering the template — only `enter` writes the file, navigating never does.
- [ ] Delete the phase-4 scaffolding list **and the phase-5 split-view widget**; the buffer moves into the centre pane.
- [ ] Decide whether the rail scrolls continuously or pages by month (design leaves it open). **→ ADR**
- [ ] *(Claude)* Tests: range query, rail ordering across scales, "note exists" marking, "captured today" contents, empty-day creation offered but not taken.

Exit: you navigate a month of logs without touching the filesystem, and the app has no list left in it.

## Phase 8 — Hybrid block editor

Goal: the block under the cursor shows source; every other block shows rendered output.

- [ ] Block segmentation from the `typst-syntax` tree (top-level markup nodes, never line-based).
- [ ] Map cursor position → active block; render inactive blocks as cached SVG fragments.
- [ ] Recompute block boundaries when the cursor leaves a block or on idle.
- [ ] *(Claude)* Tests: segmentation on multi-line constructs, cursor→block mapping at boundaries.

Exit: hybrid editing feels better than plain source for daily writing.
Pre-declared fallback (`plan.md` § Known risks): if this stalls, the logs centre pane ships as a **single pane toggling source ⇄ rendered** and this phase moves after v1.
The phase-5 split view itself is not the fallback — it dies in phase 7, because the design's centre column has no room for two panes.

## Phase 9 — Links UX

Goal: links are cheap to write and visible in both directions.

- [ ] `#l` insertion: hotkey + autocomplete over note ids/titles from the index.
- [ ] Backlinks panel on the open note.
- [ ] Dangling links visibly marked (in the panel at minimum).
- [ ] *(Claude)* Tests: autocomplete filtering, backlink query wiring.

Exit: you never type a full `#l("...")` by hand.

## Phase 10 — Capture + open-loops

Goal: zero-friction capture in, visible debt out — v0 complete.

- [ ] In-app capture: hotkey + paste → new file in `capture/` from the capture template, no required fields.
- [ ] Global (OS-level) capture strategy on Linux/Wayland: true global hotkey vs a DE shortcut launching `app --capture` through single-instance IPC. **→ ADR**
- [ ] Define what marks a capture "summarized" (suggestion: a non-empty summary block from the capture template). **→ ADR**
- [ ] Feed the phase-6 ember its real count, and make clicking it open a flat list of unsummarized captures, dangling links and typeless notes. No ages, no grouping, no per-item actions — the queries already exist from phase 3 (`adr/2026-07-debt-counter-then-list.md`).
  - **Known design gap**: the deck never drew the open-loops screen ("show the loops screen in this language" is in its own *try next*). The flat list is therefore designed here, in phase-6 vocabulary, not transcribed from a wireframe.
- [ ] *(Claude)* Tests: capture creation, summarized-detection, counter total matches the list.

Exit: v0 checklist below is fully green.

## v0 exit criteria (from `plan.md`)

- [ ] Vault structure + `#meta`/`#l` conventions + shared template (phase 1)
- [ ] File CRUD, creation from per-type templates, time notes (day/week/season) with derived prev/next movement (phase 5)
- [ ] The design language: palette and type scale as theme variables, dark **and** light, the one-line chrome (phase 6)
- [ ] The logs screen: time rail, rendered centre pane with scale chain and "captured today", month grid (phase 7)
- [ ] Hybrid block editor with live rendering — or the source ⇄ rendered toggle via the declared fallback (phases 5, 8)
- [ ] Link index, backlinks panel, dangling-link detection (phases 3, 9)
- [ ] Capture notes + open-loops ember and list (phase 10)
- [ ] `make test` green with 100% coverage; every note compiles with vanilla typst
