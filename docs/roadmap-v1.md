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

- [ ] Ctrl+P opens the palette; typing filters; Enter runs; Escape closes and restores focus — the link-picker interaction grammar.
- [ ] The command set at birth: the chords the app already answers (theme toggle, capture, loops list, link picker, time movement…), each under a plain English name; the exact list and labels. **→ ADR**
- [ ] The palette stays complete by construction: every later phase that adds a keystroke adds its palette entry in the same change — a line item in each phase's work, not an audit at the end.
- [ ] The overlay's shape and metrics (undrawn in the deck). **→ ADR**
- [ ] Tests: filter narrows to the match, Enter dispatches the command, Escape restores focus, the registered set matches the app's chords.

Exit: every chord the app answers is also reachable by name through Ctrl+P.

## Phase 1 — Positions store (Rust, no UI)

Goal: positions have a home an index rebuild cannot touch — `adr/2026-07-positions-separate-file.md` becomes code before any card exists to sit on it.

- [ ] The file lives under `.index/`, beside the database, never inside it.
  - [ ] Format and filename (suggestion: something human-readable — this is user data and should be debuggable at 3 AM). **→ ADR**
- [ ] Semantics: id → (x, y); a missing entry means "not yet placed" — never an error, phase 8's auto-placement decides later.
  - [ ] What happens to the position of a deleted note: tombstone vs drop (recreating an id is possible). **→ ADR**
- [ ] Read once when the table loads; written on move, debounced (the v0 phase-5 idle-timer pattern).
- [ ] Tests: roundtrip, unknown ids tolerated on load, a malformed file degrades to "nothing placed" rather than a crash, and the invariant test — rebuild the index database from scratch, every position intact.

Exit: positions survive a full index rebuild, enforced by a test — the invariant is structural, not remembered.

## Phase 2 — Walking skeleton: the table at rest

Goal: the table icon stops being dim — permanent, capture and generated notes render as positioned cards at titles zoom, pannable, draggable.

Spec: wireframe state 6a. `Screen::Table` finally mounts (the "table mounts in v1" note in `ui.rs` ends here); the links-footer and Ctrl+Enter "wait for v1" branches end in phase 3, not here.
The real vault is small at this point — the fixture vault carries density in tests; the deck's undrawn "titles zoom, dense (30 cards)" state arrives organically with use.

- [ ] The canvas: void background, the faint star field, pan by dragging the void.
- [ ] Cards at titles zoom: ~176px wide (mockup 170–180, normalized to ×4), uppercase mono type label over a sans title, absolutely positioned from phase 1; unplaced notes stack somewhere deterministic and visible — dumb and honest until phase 8.
- [ ] Note kinds at a glance:
  - [ ] permanent: filled card + 3px type bar — the six wireframe hues plus organisation and personal from the turn-1 reference, as new `theme.css` variables;
  - [ ] **Known design gap**: the wireframes leave most light-mode table colours undrawn (the "—" cells in the palette table) — derive the light siblings in the same pass, both themes from the first rule as always. **→ ADR**
  - [ ] capture: dimmer fill, grey bar, muted title, **age in the label** ("capture · 3 d") — the friction age surfaces here, per `plan.md` § Friction system;
  - [ ] generated: dashed border all round, no hue. Canvas styling plus the existing model rules (linkable-from, disposable) is what "`generated` type defined" means; a `generated.typ` template is deliberately *not* written — nothing creates generated notes by hand, and the pipeline that will is v3.
- [ ] Drag a card to move it; the position persists through phase 1's debounced write.
- [ ] The index query: everything except time notes — time is the one category that never appears on the table (`plan.md` § Note model).
- [ ] The watcher keeps the table live as it does the rail: notes created or edited outside the app appear and repaint without a relaunch.
- [ ] Tests: query excludes time notes, kind → card treatment, drag reaches the store, unplaced fallback is deterministic.

Exit: captures and fixture-style permanent notes are on the table; drag a card, quit, relaunch — it stayed put.

## Phase 3 — The writing sheet

Goal: click a card, write at the size of a page — permanent notes become editable in-app for the first time, ending `adr/2026-07-permanent-notes-wait-for-table.md`.

Spec: wireframe state 6b. The editor itself is done (v0 phase 8) — this phase is the surface around it.

- [ ] The sheet: a tall panel beside its card with the sheet fill, border and soft glow; the table dims under an overlay; the origin card keeps a brighter border and a lit tether edge runs card → sheet — the tether keeps place legible.
  - [ ] `sheetW` and `dimOpacity` are the deck's open knobs — pick by feel once it runs, then freeze. **→ ADR**
- [ ] Sheet content, top to bottom: the note's rendered meta line, the hybrid block editor (the same `Editor`/blocks machinery the logs centre pane mounts), the backlinks-only footer ("← 2").
- [ ] The links footer and Ctrl+Enter stop being inert for permanent targets: a backlink or an `#l` under the caret opens that card's sheet (the "wait for v1's table" branches from v0 phase 9 end here; extends `adr/2026-08-ctrl-enter-opens-time-links.md`).
- [ ] Escape closes the sheet and puts the card back; autosave and the Ctrl+Q flush already live below the widget and must simply keep holding.
- [ ] Tests: open/close state, tether endpoints track the card, editor wiring through autosave → watcher → index, permanent-target links open sheets.

Exit: a permanent note is opened, edited and closed entirely in the sheet — reading and writing knowledge no longer leaves the app.

## Phase 4 — CRUD for permanent notes

Goal: create and delete without leaving the table — **the vault starts growing here, and the manual zettelkasten workflow retires; start daily-driving the table at the end of this phase**.

On a from-scratch vault this phase is load-bearing twice over: creation is the only way permanent notes come to exist (capture aside), and rewriting an old note by hand when it's needed — the cherry-pick path from `adr/2026-08-vault-starts-from-scratch.md` — is exactly this create-and-write gesture.

- [ ] Create: a keystroke summons a type picker (the link-picker overlay pattern — the design has no buttons), instantiates the per-type template (v0 phase-5 machinery), and opens the new card's sheet.
  - [ ] The keystroke, the picker's shape, and where the new card lands before phase 8 auto-places it (suggestion: viewport centre). **→ ADR**
- [ ] Delete from the sheet: unconfirmed, no trash (`adr/2026-07-delete-unconfirmed-no-trash.md` now covers permanent notes); the dangling links it causes surface through the loops list, as designed.
- [ ] Capture promotion is editing, not a feature: set `type` in `#meta`, write the summary — the card regains a hue and full fill when the watcher re-indexes it. Verify that round-trip repaints; decide whether any affordance beyond the editor is needed (suggestion: none). **→ ADR** only if one is.
- [ ] Tests: create writes the file and the card appears, delete removes both, promotion recolours through the watcher.

Exit: a new permanent note goes from keystroke to written note without touching the filesystem; Obsidian and the manual zettelkasten are both fully replaced.

## Phase 5 — Constellations: link edges

Goal: real links drawn as solid edges with star nodes — the vault becomes legible as a graph.

- [ ] Solid edges between placed cards from the link index; small node dots where an edge meets a card; edges drawn under cards, straight lines, no routing.
- [ ] Edges follow drags live; the watcher refresh redraws them when links change in the files.
- [ ] Links to unplaced or nonexistent notes draw nothing — dangling debt stays in the loops list, off the canvas; dashed proposed edges are v3 and need only the drawing layer to leave room, not code.
- [ ] Tests: edge set mirrors the link index, endpoints track a drag, dangling draws nothing.

Exit: the table reads as a constellation, and dragging a card drags its edges.

## Phase 6 — Semantic zoom: bodies

Goal: two levels — titles ⇄ rendered typst bodies — behind a keystroke, fast over the whole vault.

- [ ] Body zoom renders each card's cached SVG (the v0 phase-4 render cache; the template's own typography, never restyled by the app).
  - [ ] Card metrics at body zoom, and the keystroke: the deck never drew this state — designed here, in the phase-6-v0 vocabulary, like the loops list was. **→ ADR**
- [ ] Viewport culling: only visible cards render at either level — the "low thousands" scale answer (`plan.md` § Canvas).
- [ ] Tests: toggle state, culling boundary math, off-viewport cards render nothing.

Exit: toggling zoom over the fixture vault's densest cluster shows no visible jank.

## Phase 7 — Findability: filters and jump

Goal: any card in seconds — filter by tag and type, jump by name.

- [ ] Filters by tag and by type; filtered-out cards **dim, not disappear** — spatial memory is the point, and holes would break the map.
  - [ ] The summoning keystroke and the overlay's shape (link-picker pattern again). **→ ADR**
- [ ] Jump-to-note: search over ids and titles (the v0 phase-9 autocomplete query), pan/zoom the viewport to the card.
- [ ] Tests: the dimmed set matches the query, jump resolves and centres.

Exit: tag, type and name each reach any card without panning by hand.

## Phase 8 — Auto-placement and cluster arrange

Goal: new notes land near their links; layout is only ever a command, never a background process.

- [ ] Auto-place an unplaced note near its strongest linked placed card; deterministic and bounded — no iteration to convergence at place time.
  - [ ] What "strongest" means (link count? both directions?) and the fallback for an unlinked note. **→ ADR**
- [ ] On-demand "arrange this cluster" — at most a command, force-directed is never the default (`plan.md` § Canvas).
  - [ ] What "this cluster" scopes to, the keystroke, and the explicit iteration bound. **→ ADR**
- [ ] The invariant, test-enforced: a hand-placed card is never moved by either mechanism — positions change only under the user's drag or their explicit arrange.
- [ ] Tests: placement near links, unlinked fallback, the hand-placed invariant, arrange terminates within its bound.

Exit: creating a linked note lands it where it belongs, and nothing ever moves a card you placed.

## v1 exit criteria (from `plan.md` and `adr/2026-07-permanent-notes-wait-for-table.md`)

- [ ] Command palette: every command reachable by name via Ctrl+P (phase 0)
- [ ] Canvas with persistent positions that survive index rebuilds (phases 1–2)
- [ ] Two-level semantic zoom: titles ⇄ rendered bodies (phase 6)
- [ ] Tethered writing sheet on card click, dimmed table behind it (phase 3)
- [ ] Tag & type filters, type colours, jump-to-note (phases 2, 7)
- [ ] Auto-placement of new notes near linked ones; on-demand cluster arrange (phase 8)
- [ ] Capture/generated visual treatment; `generated` type defined (phase 2)
- [ ] Permanent-note CRUD in the app; the vault grows without leaving it (phases 3–4)
- [ ] `make test` green with 100% coverage; every note compiles with vanilla typst (`make check-vault`)
- [ ] The ceiling held: nothing shipped in v1 beyond this list (`plan.md` § Known risks 4)
