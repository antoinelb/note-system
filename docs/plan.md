# Plan: Personal Typst Knowledge System

Desktop app for Linux, built with Dioxus, storing notes as plain typst files. Replaces Obsidian (daily notes) and a manual typst zettelkasten. Single user, no accounts, no plugins — extended by editing the source.

## Design principles

1. **Plain files are the source of truth.** Every note is a `.typ` file on disk, compilable by the vanilla typst CLI. The app maintains a derived index (links, tags, metadata) that can always be rebuilt from the files. This is also what makes the vault directly usable by Claude Code with zero integration work.
2. **The index-card table.** The spatial canvas is the core mental model: cards with *persistent* positions on an infinite table, spatial memory as a first-class navigation tool. Reach out to a card, write on it, put it back.
3. **Asymmetric friction.** Zero friction for capturing and writing. Soft, visible friction for accumulation debt (unsummarized captures, unresolved link suggestions, dangling links). Deliberate friction for delegating thinking to AI: the AI suggests, the human writes.
4. **AI never writes prose in my notes.** Exception: the explicit `generated` note type. Suggestions live in a sidecar index, never in note files.
5. **No hard blocks.** All friction mechanisms are soft (visible debt), never save-blockers.

## Note model

### Categories

| Category | Purpose | On canvas? |
|---|---|---|
| Permanent | The knowledge system proper | Yes |
| Time-based | Daily/weekly logs, tasks, quotes | No (excluded) |
| Capture | Zero-friction dumps: pastes, sources, quick ideas | Yes, visually second-class (grayed) until promoted |
| Generated | AI-written digests (e.g. wiki from articles) | Yes, visually distinct, disposable |

### Permanent note types

`person`, `organisation`, `source`, `concept`, `claim`, `idea`, `personal`, `project`.

- `concept` vs `claim`: a concept is a definition/abstraction; a claim has a truth value and needs provenance.
- `idea` vs `personal`: `personal` holds goals, future plans, life notes — things that are obviously mine but resist categorization. `idea` is a subjective proposition about the world.
- Provenance is a **field**, not a type: `origin: self` or `origin: <source-id>`. Applies at least to `claim` and `idea`.

Directories encode only the four **categories** (permanent, time-based, capture, generated); within `permanent/` all types live flat. The type itself is a **`#meta` field**. Metadata lives in a `#meta(..)` call at the top of each file — a typst function defined in a shared vault template so files compile standalone. Fields are minimal: `id`, `type`, `created`, `tags`, plus `origin` where relevant. Per-type extra fields only when a real need appears. Consequence: changing a note's type is a one-field edit, not a file move, and the index (not the filesystem) is the authority for querying by type.

### Time-based notes

Daily, weekly and season notes follow the current Obsidian pattern: quotes section (manually written), task list, notes, "what I learned today". Created from a **template that is itself a note** — editable like any other file, one template per note type, filled from a closed set of `{{id}}`/`{{created}}`/`{{title}}`/`{{content}}` placeholders (`adr/2026-07-template-placeholders-closed-set.md`). No task engine in v0: tasks are just text; no recurrence, aggregation, or carryover logic yet. Time notes link *to* permanent notes but are excluded from the canvas. All three scales are v0, because the logs screen shows them side by side; the day/week/season chain **and prev/next movement** are derived from the id convention (`2026-07-23`, `2026-w30`, `2026-summer`) plus an index existence check, never from links stored in the file — the templates seed no navigation (`adr/2026-07-time-navigation-derived-not-stored.md`).

### Capture notes

Created instantly, two ways: a desktop shortcut pipes a paste into `app --capture`, a short-lived headless process that writes the file and exits (`adr/2026-08-capture-headless-second-process.md`), and Ctrl+Shift+V does the same from inside the app. Ids are the clock to the second (`adr/2026-08-capture-timestamp-ids.md`). No required fields, no summary required at creation. Debt mechanism: a capture whose `== Summary` section is still empty (`adr/2026-08-summarized-nonempty-summary-section.md`) appears in the open-loops list and stays gray on the canvas. Promotion to permanent = choosing a type + writing your own summary/prose. The pasted original is retained (e.g. in an appendix block) after promotion.

## Storage layout

```
vault/
  templates/            # template.typ (shared defs: #meta, #l, styles) + per-type note templates (editable notes)
  time/                 # time-based (daily, weekly, season — kind is a #meta type)
  permanent/            # all permanent types, flat; type is a #meta field
  capture/
  generated/
  .index/               # derived: SQLite (links, tags, positions, suggestions)
```

- Link syntax: `#l("note-id")` — valid typst, compiles standalone. Inserted via hotkey/autocomplete so it's cheap to type.
- Canvas positions and AI suggestions live in `.index/`, never in note files. Positions live in their own file, separate from the index database, so the database stays disposable and rebuilding it cannot lose them (`adr/2026-07-positions-separate-file.md`).
- Index is rebuilt by parsing files; file watcher keeps it live.

## Screens

Design direction and wireframes: `design/wireframes-v0.md` (layout, palette, chrome and states; a few knobs — sheet width, dim, seasons — stay open until it runs). Decision: `adr/2026-07-two-screens-table-and-logs.md`.
**On any conflict between this section and the wireframes, the wireframes win** (`adr/2026-07-plan-realigned-with-wireframes.md`).

The app has exactly two screens, one 14×14 stroked icon each way — no text labels:

1. **The table** — the spatial canvas of permanent notes. Where knowledge lives. **v1.**
2. **The logs** — day, week and season notes on one screen: a time rail on the left where indentation carries the scale, the selected note rendered in the centre with its clickable scale chain above it and a "captured today" block below it listing that day's captures and generated notes, and a jump panel on the right (a month grid whose day cells encode existence, whose week-number gutter is clickable, and with a season row below it). This is the only calendar in the app. **v0.**

Chrome is "one line": the two screen icons left, the open-loops ember right, nothing else.
The palette is "Deep field" — an indigo void, not greyscale: link edges are constellations with star nodes, and the one warm element is the amber ember and caret.
**Dark is the main mode and light is a first-class sibling**; every colour goes through a theme variable from the first styled rule (`adr/2026-07-design-language-own-phase.md`).
Note bodies are rendered typst — the app never restyles them, the meta line is the note's own `#meta` output, and the design's prose typography is a `template.typ` requirement rather than an app one.
Navigating never creates a file: selecting an empty day shows one line offering the template, and only `enter` writes it.

Both screens carry the open-loops ember in the top line; clicking it opens a flat list (`adr/2026-07-debt-counter-then-list.md`).
**Zero loops = zero indicator** — the number is absent, not zeroed. Absence is the reward.

## Editor

- **Hybrid block editing from v0**: the block containing the cursor shows raw typst source; all other blocks show rendered output. Per-block (not per-line) because typst constructs span lines and only compile as complete expressions.
- Rendering: embed the typst compiler (Rust crate) — compile note → SVG, cache per note, invalidate on edit. Fast enough at this scale.
- **Vim later, but architected for now**: keep a clean separation between the text buffer/edit-command layer and the widget layer, so a modal keymap can be inserted without rewriting the editor. No mature code-editor widget exists for Dioxus; the editor is built on primitives and is the highest-effort component of the project.
- Fallback if hybrid editing stalls: the logs centre pane as a single pane toggling source ⇄ rendered.
  The phase-5 split view was never the fallback — it died with the phase-7 logs screen, whose centre pane ships **read-only** until the hybrid editor (or its toggle fallback) lands (`adr/2026-07-logs-centre-read-only.md`); writing happens outside the app in that window.

## Canvas (the table)

- **Persistent positions.** Every canvas note stores x/y. New notes auto-place near their strongest links; once moved by hand, position is permanent. Force-directed layout is at most an on-demand "arrange this cluster" command, never the default.
- **Cards** are flat rectangles — no paper texture, no rotation. Type is a thin colour bar on the left edge, and that is the only colour on the screen.
- **Semantic zoom, two levels.** Titles (default) and rendered typst body (cached SVGs). The "coloured dots" level is dropped. Viewport culling makes thousands of notes a non-issue for rendering.
- **Enter/exit nodes.** Clicking a card opens a **writing sheet** — a tall panel, far larger than the card, that opens *beside* it with a line tethering it back. The rest of the table dims; the origin card and its tether stay lit. The sheet is chromeless and keeps exactly two lines of its own: the note's rendered `#meta` line at the top and a footer showing only backlinks ("← 2") — the sheet shows the note's own title, not the card's header, and everything else is a keystroke. Escape puts the card back. Writing happens at the size of a page while the sense of place stays visible around it.
- **Findability**: filter by tag and by type (filtered-out cards dim, not disappear), plus jump-to-note search that pans/zooms to the card.
- **Edges**: real links (`#l(..)`) as solid edges; AI-suggested links as dashed edges (see below).
- Scale target: hundreds now, low thousands eventually.

## Friction system

One **open-loops panel**, always accessible, showing three kinds of debt:

1. Captures without a self-written summary.
2. AI-suggested links neither written nor dismissed.
3. Dangling `#l(..)` references to notes that don't exist.
4. Notes with missing or unparseable `#meta` (typeless notes) — load-bearing now that type lives only in metadata, not in the path.

Nothing blocks saving or writing; the debt is simply always visible.
The list itself carries no ages, no grouping and no per-item actions (`adr/2026-07-debt-counter-then-list.md`) — age surfaces on the table instead, in the capture card's own label ("capture · 3 d"), which is v1.
Additional soft incentives can be layered later (e.g. counts on the canvas), but the panel is the mechanism.

## AI integration

Two separate mechanisms:

1. **In-app**: the app shells out to the `claude` CLI headlessly for tagging and link suggestion over the vault.
2. **Vault-as-context**: an MCP server exposing the vault (search, read, link graph) so any Claude Code session can use the notes for projects, study, research.

### Suggested links — the core friction design

- Suggestions are stored in the sidecar index only. Discovery is free and ambient and never interrupts: dashed edges with a hollow star on the canvas, and — inside a note, on either screen — a single dashed line at the *end of the page* ("proposed · evergreen-notes → this note", with the app's only visible hint, "enter accept · x dismiss").
- **Accepting = writing.** No accept button that edits the file for you. While editing a note, a relevant suggestion appears as gray ghost text at the cursor — press Tab to insert the `#l(..)` (code-completion style). You are still the one writing, in your own sentence, at a place you chose.
- Soft incentive (not requirement) to embed the link in a phrase saying *why* the connection exists — e.g. the ghost text nudges toward sentence context rather than bare link insertion. Exact mechanism TBD during implementation; must not add typing overhead beyond the Tab.
- Writing a suggested link (by Tab or manually) clears the suggestion; dismissing it clears it too. Un-actioned suggestions accumulate as visible debt.

### Tagging

AI proposes tags into the index; applying them to `#meta(..)` goes through the same explicit, low-cost confirm flow. (Tags are metadata, not prose, so a lighter touch than links is acceptable — but still never auto-applied.)

### Generated notes

Type exists from v1 (directory, canvas styling, model rules). Actual generation pipeline (e.g. digest a set of papers into a wiki) implemented later. Generated notes may be linked *from*, are visually marked, and are treated as disposable/regenerable — they are context, not knowledge.

## Roadmap

**v0 — daily driver for writing** (task breakdown and current state: `roadmap-v0.md`)
- Vault structure, `#meta` / `#l` conventions, shared template
- File CRUD, note creation from per-type templates, time notes (day/week/season) with derived prev/next movement
- The design language: "Deep field" palette and type scale as theme variables, dark and light, the one-line chrome
- The logs screen: time rail, rendered centre pane with scale chain and "captured today", month grid
- Hybrid block editor with live typst rendering (split-view fallback)
- Link index, backlinks panel, dangling-link detection
- Capture notes (desktop shortcut into `app --capture`, or Ctrl+Shift+V in the app) + open-loops counter opening a flat list (captures + dangling links + typeless notes)

**v1 — the table**
- Canvas with persistent positions, two-level semantic zoom (titles → rendered bodies)
- Tethered writing sheet on card click, dimmed table behind it; tag & type filters; type colors; jump-to-note
- Auto-placement of new notes near linked ones; on-demand cluster arrange
- Capture/generated visual treatment; `generated` type defined

**v2 — vim**
- Modal editing layer on the existing buffer architecture, implemented incrementally as needed

**v3 — AI**
- `claude` CLI integration: tag proposals, link suggestions
- Dashed suggestion edges, ghost-text Tab-completion, suggestion debt in open-loops panel
- MCP server exposing the vault to Claude Code
- (later) generated-note pipeline

**Later (unscheduled)**
- Auto-rename: explicit action that re-derives the id from the current title, renames the file, and rewrites all inbound `#l` links via the index (ids stay frozen on ordinary title edits; see `adr/2026-07-id-scheme-kebab-frozen.md`)

## Known risks

1. **The editor is half the project.** Hybrid block editing + typst rendering + (later) vim on Dioxus primitives, with no existing editor widget. Mitigation: split-view fallback, strict buffer/UI separation, per-block rather than per-character ambitions. The fallback is narrower than it first looked: split view needs a full-window editor, and the design gives it one nowhere — v0's logs screen puts the editor in a three-pane centre column, v1 puts it in the writing sheet. So the split view is phase-5 scaffolding with a known deletion date, and hybrid editing stops being optional as soon as the logs screen lands, not when the table does.
2. **Typst parsing for the index.** Extracting `#meta` and `#l` calls needs a real parse (the `typst-syntax` crate), not regex, to survive edge cases.
3. **Canvas position durability.** Positions are user data disguised as index data — must not be lost on index rebuilds.
4. **Scope creep at v1.** The canvas is where feature ideas multiply; the v1 list above is the ceiling, not the floor.
