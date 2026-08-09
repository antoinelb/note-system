# v1 Roadmap — the table

Task breakdown of `plan.md` § Roadmap, v1.
The plan carries the *what* and *why*; this file carries the *order* and the *state*.
Ordering rationale: `adr/2026-08-v1-walking-skeleton-order.md`, amended by `adr/2026-08-command-palette-first-in-v1.md` (the palette leads).
UI direction: `design/wireframes-v0.md` § The table screen (states 6a–6b); on any conflict with the plan, the wireframes win (`adr/2026-07-plan-realigned-with-wireframes.md`).
The vault starts from scratch — no migration, old notes are rewritten by hand when actually needed (`adr/2026-08-vault-starts-from-scratch.md`) — so the table begins nearly empty and fills through in-app creation.
**The v1 list is the ceiling, not the floor** (`plan.md` § Known risks 4): the canvas is where feature ideas multiply, and anything not below waits for its own version or the polish backlog.

## How we work

The v0 loop, unchanged (`roadmap-v0.md` § How we work):

- The only goal is building the note system (`adr/2026-07-goal-build-only.md`).
- Items marked **→ ADR** are decisions to take together at the start of the task, then record in `docs/adr/`.
- A phase is done when its exit criterion holds and `make test` passes with 100% coverage; a shortfall on regions is diagnosed per instantiation before writing any test (`adr/2026-07-coverage-100-percent-lines.md`).
- Per-task loop: discuss approach → decisions become ADRs → implement with tests until green → commit.
- Read `.claude/dioxus.md` before any Dioxus code (every time).
- The v0 polish backlog (`roadmap-v0.md`) stays where it is — v1 does not absorb it; items get pulled when daily-driving demands them.

## Phase 0 — Command palette

Goal: every command reachable by name — Ctrl+P summons a fuzzy-searched list of the app's commands, Obsidian-style.

Spec: the deck never drew a palette — designed here in the phase-6-v0 vocabulary, like the loops list was, on the link-picker overlay pattern (the design has no buttons).

- [x] Ctrl+P opens the palette; typing filters; Enter runs; Escape closes and restores focus — the link-picker interaction grammar.
- [x] The command set at birth: the chords the app already answers (theme toggle, capture, loops list, link picker, time movement…), each under a plain English name; the exact list and labels. **→ ADR** (`adr/2026-08-palette-birth-command-list.md`)
- [x] The palette stays complete by construction: every later phase that adds a keystroke adds its palette entry in the same change — a line item in each phase's work, not an audit at the end.
- [x] The overlay's shape and metrics (undrawn in the deck). **→ ADR** (`adr/2026-08-command-palette-overlay-shape.md`)
- [x] Tests: filter narrows to the match, Enter dispatches the command, Escape restores focus, the registered set matches the app's chords.

Exit: every chord the app answers is also reachable by name through Ctrl+P.

## Phase 1 — Positions store (Rust, no UI)

Goal: positions have a home an index rebuild cannot touch — `adr/2026-07-positions-separate-file.md` becomes code before any card exists to sit on it.

- [x] The file lives under `.index/`, beside the database, never inside it.
  - [x] Format and filename (suggestion: something human-readable — this is user data and should be debuggable at 3 AM). **→ ADR** (`adr/2026-08-positions-plain-lines-file.md`)
- [x] Semantics: id → (x, y); a missing entry means "not yet placed" — never an error, phase 8's auto-placement decides later.
  - [x] What happens to the position of a deleted note: tombstone vs drop (recreating an id is possible). **→ ADR** (`adr/2026-08-position-dropped-on-delete.md`)
- [x] Read once when the table loads; written on move, debounced (the v0 phase-5 idle-timer pattern) — the store is synchronous like `Editor::save`; phase 2's drag handler owns the timer and the Ctrl+Q flush.
- [x] Tests: roundtrip, unknown ids tolerated on load, a malformed file degrades to "nothing placed" rather than a crash, and the invariant test — rebuild the index database from scratch, every position intact.

Exit: positions survive a full index rebuild, enforced by a test — the invariant is structural, not remembered.

## Phase 2 — Walking skeleton: the table at rest

Goal: the table icon stops being dim — permanent, capture and generated notes render as positioned cards at titles zoom, pannable, draggable.

Spec: wireframe state 6a. `Screen::Table` finally mounts (the "table mounts in v1" note in `ui.rs` ends here); the links-footer and Ctrl+Enter "wait for v1" branches end in phase 3, not here.
The real vault is small at this point — the fixture vault carries density in tests; the deck's undrawn "titles zoom, dense (30 cards)" state arrives organically with use.

- [x] The canvas: void background, the faint star field, pan by dragging the void.
- [x] Cards at titles zoom: ~176px wide (mockup 170–180, normalized to ×4), uppercase mono type label over a sans title, absolutely positioned from phase 1; unplaced notes stack somewhere deterministic and visible — dumb and honest until phase 8 (origin grid: `adr/2026-08-table-mounts-titles-zoom.md`).
- [x] Note kinds at a glance:
  - [x] permanent: filled card + 3px type bar — the six wireframe hues plus organisation and personal from the turn-1 reference, as new `theme.css` variables;
  - [x] **Known design gap**: the wireframes leave most light-mode table colours undrawn (the "—" cells in the palette table) — derive the light siblings in the same pass, both themes from the first rule as always. **→ ADR** (`adr/2026-08-light-table-colours-derived.md`)
  - [x] capture: dimmer fill, grey bar, muted title, **age in the label** ("capture · 3 d") — the friction age surfaces here, per `plan.md` § Friction system;
  - [x] generated: dashed border all round, no hue. Canvas styling plus the existing model rules (linkable-from, disposable) is what "`generated` type defined" means; a `generated.typ` template is deliberately *not* written — nothing creates generated notes by hand, and the pipeline that will is v3.
- [x] Drag a card to move it; the position persists through phase 1's debounced write.
- [x] The index query: everything except time notes — time is the one category that never appears on the table (`plan.md` § Note model); id-less notes stay off it too, positions being keyed by id (`adr/2026-08-table-mounts-titles-zoom.md`).
- [x] The watcher keeps the table live as it does the rail: notes created or edited outside the app appear and repaint without a relaunch.
- [x] Screen switching, undrawn in the deck: chrome icons click, Ctrl+1/Ctrl+2, and "go to table" / "go to logs" in the palette. **→ ADR** (`adr/2026-08-screen-switch-gesture.md`)
- [x] Tests: query excludes time notes, kind → card treatment, drag reaches the store, unplaced fallback is deterministic.

Exit: captures and fixture-style permanent notes are on the table; drag a card, quit, relaunch — it stayed put.

## Phase 3 — The writing sheet

Goal: click a card, write at the size of a page — permanent notes become editable in-app for the first time, ending `adr/2026-07-permanent-notes-wait-for-table.md`.

Spec: wireframe state 6b. The editor itself is done (v0 phase 8) — this phase is the surface around it.

- [x] The sheet: a tall panel beside its card with the sheet fill, border and soft glow; the table dims under an overlay; the origin card keeps a brighter border and a lit tether edge runs card → sheet — the tether keeps place legible (`adr/2026-08-sheet-stacking-dom-order.md`; every card kind opens one, `adr/2026-08-every-card-opens-the-sheet.md`; click vs drag, `adr/2026-08-click-opens-drag-moves.md`).
  - [x] `sheetW` and `dimOpacity` are the deck's open knobs — frozen at the deck defaults (sheet 440+620 wide, dim 0.4): no feel signal exists before daily-driving, and reopening is a one-line change. **→ ADR** (`adr/2026-08-sheet-knobs-frozen-at-deck-defaults.md`)
- [x] Sheet content, top to bottom: the note's rendered meta line, the hybrid block editor (the same `Editor`/blocks machinery the logs centre pane mounts — literally the same signal, `adr/2026-08-sheet-reuses-the-one-editor.md`), the backlinks-only footer ("← 2").
- [x] The links footer and Ctrl+Enter stop being inert for permanent targets: a backlink or an `#l` under the caret opens that card's sheet (the "wait for v1's table" branches from v0 phase 9 end here; extends `adr/2026-08-ctrl-enter-opens-time-links.md` via `adr/2026-08-permanent-links-open-sheets.md`).
- [x] Escape closes the sheet and puts the card back; autosave and the Ctrl+Q flush already live below the widget and must simply keep holding.
- [x] Tests: open/close state, tether endpoints track the card, editor wiring through autosave → watcher → index, permanent-target links open sheets.

Exit: a permanent note is opened, edited and closed entirely in the sheet — reading and writing knowledge no longer leaves the app.

## Phase 4 — CRUD for permanent notes

Goal: create and delete without leaving the table — **the vault starts growing here, and the manual zettelkasten workflow retires; start daily-driving the table at the end of this phase**.

On a from-scratch vault this phase is load-bearing twice over: creation is the only way permanent notes come to exist (capture aside), and rewriting an old note by hand when it's needed — the cherry-pick path from `adr/2026-08-vault-starts-from-scratch.md` — is exactly this create-and-write gesture.

- [x] Create: Ctrl+N summons a two-step type-then-title overlay (the link-picker pattern), instantiates the per-type template, and opens the new card's sheet.
  - [x] The keystroke (Ctrl+N), the picker's shape (one overlay, two steps), and where the new card lands (viewport centre, injected size). **→ ADR** (`adr/2026-08-ctrl-n-two-step-create-overlay.md`, `adr/2026-08-new-card-lands-at-viewport-centre.md`)
- [x] Delete from the sheet: unconfirmed, no trash — a palette-only command, no chord (`adr/2026-08-delete-note-palette-only-from-sheet.md` extends `adr/2026-07-delete-unconfirmed-no-trash.md`); the dangling links it causes surface through the loops list, as designed.
- [x] Capture promotion is editing, not a feature: set `type` in `#meta`, write the summary — the card regains a hue and full fill when the watcher re-indexes it. No affordance beyond the editor; the treatment rule needed code (`adr/2026-08-typed-capture-wears-its-hue.md`).
- [x] Tests: create writes the file and the card appears, delete removes both, promotion recolours through the watcher.

Exit: a new permanent note goes from keystroke to written note without touching the filesystem; Obsidian and the manual zettelkasten are both fully replaced.

## Phase 5 — Constellations: link edges

Goal: real links drawn as solid edges with star nodes — the vault becomes legible as a graph.

- [x] Solid edges between placed cards from the link index; small node dots where an edge meets a card; edges drawn under cards, straight lines, no routing (`adr/2026-08-edges-svg-under-cards.md`).
- [x] Edges follow drags live; the watcher refresh redraws them when links change in the files.
- [x] Links to unplaced or nonexistent notes draw nothing — dangling debt stays in the loops list, off the canvas; dashed proposed edges are v3 and need only the drawing layer to leave room, not code.
- [x] Tests: edge set mirrors the link index, endpoints track a drag, dangling draws nothing.

Exit: the table reads as a constellation, and dragging a card drags its edges.

## Phase 6 — Semantic zoom: bodies

Goal: two levels — titles ⇄ rendered typst bodies — behind a keystroke, fast over the whole vault.

- [x] Body zoom renders each card's cached SVG (a second cache, `BodyCache`, watcher-invalidated — the phase-4 fragment cache's sweep policy would evict the table, `adr/2026-08-body-cache-per-note-svg.md`; the template's own typography, never restyled by the app).
  - [x] Card metrics at body zoom (scale 3, 176px logical cards, 240px clipped bodies), and the keystroke (Ctrl+= / Ctrl+-): designed in the phase-6-v0 vocabulary. **→ ADR** (`adr/2026-08-body-zoom-scale-and-metrics.md`)
- [x] Viewport culling: only visible cards render at either level — the "low thousands" scale answer (`plan.md` § Canvas; size from onresize, `adr/2026-08-viewport-culling-onresize.md`).
- [x] Tests: toggle state, culling boundary math, off-viewport cards render nothing.

Exit: toggling zoom over the fixture vault's densest cluster shows no visible jank.

## Phase 7 — Findability: filters and jump

Goal: any card in seconds — filter by tag and type, jump by name.

- [x] Filters by tag and by type; filtered-out cards **dim, not disappear** — spatial memory is the point, and holes would break the map; the active filter names itself in the chrome.
  - [x] The summoning keystroke (Ctrl+F) and the overlay's shape (one query over tags + the eight types; empty-Enter clears). **→ ADR** (`adr/2026-08-filter-overlay-ctrl-f.md`)
- [x] Jump-to-note: Ctrl+O, search over ids and titles (the v0 phase-9 autocomplete query restricted to carded notes), pan the viewport to centre the card at the current zoom (`adr/2026-08-jump-ctrl-o-centres-viewport.md`).
- [x] Tests: the dimmed set matches the query, jump resolves and centres.

Exit: tag, type and name each reach any card without panning by hand.

## Phase 8 — Auto-placement and cluster arrange

Goal: new notes land near their links; layout is only ever a command, never a background process.

- [x] Auto-place an unplaced note near its strongest linked placed card; deterministic and bounded — a derivation-time proposal on a ring of free slots, never a store write, so the card follows its links live until a drag pins it; creation's viewport-centre stamp became a session birth slot.
  - [x] "Strongest" = edge count in both directions, ties to the smallest anchor id; an unlinked note keeps the origin grid. **→ ADR** (`adr/2026-08-auto-place-strongest-link-ring.md`)
- [x] On-demand "arrange this cluster" — a palette-only command over the open sheet's connected component, force-directed never the default (`plan.md` § Canvas).
  - [x] The scope (the sheet's component), the gesture (palette-only, no chord), and the explicit bound (50 clamped iterations, flat). **→ ADR** (`adr/2026-08-arrange-cluster-command.md`)
- [x] The invariant, test-enforced *and structural*: a hand-placed card is never moved by either mechanism — auto-placement cannot write the store at all, so positions change only under the user's drag or their explicit arrange.
- [x] Tests: placement near links, unlinked fallback, the hand-placed invariant, arrange terminates within its bound.

Exit: creating a linked note lands it where it belongs, and nothing ever moves a card you placed.

## v1 exit criteria (from `plan.md` and `adr/2026-07-permanent-notes-wait-for-table.md`)

- [x] Command palette: every command reachable by name via Ctrl+P (phase 0)
- [x] Canvas with persistent positions that survive index rebuilds (phases 1–2)
- [x] Two-level semantic zoom: titles ⇄ rendered bodies (phase 6)
- [x] Tethered writing sheet on card click, dimmed table behind it (phase 3)
- [x] Tag & type filters, type colours, jump-to-note (phases 2, 7)
- [x] Auto-placement of new notes near linked ones; on-demand cluster arrange (phase 8)
- [x] Capture/generated visual treatment; `generated` type defined (phase 2)
- [x] Permanent-note CRUD in the app; the vault grows without leaving it (phases 3–4)
- [x] `make test` green with 100% coverage; every note compiles with vanilla typst (`make check-vault`)
- [x] The ceiling held: nothing shipped in v1 beyond this list (`plan.md` § Known risks 4)
