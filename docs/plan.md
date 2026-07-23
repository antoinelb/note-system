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

Daily (and later weekly) notes follow the current Obsidian pattern: prev/next navigation links, quotes section (manually written), task list, notes, "what I learned today". Created from a **template that is itself a note** — editable like any other file, one template per note type. No task engine in v0: tasks are just text; no recurrence, aggregation, or carryover logic yet. Daily notes link *to* permanent notes but are excluded from the canvas.

### Capture notes

Created instantly via global hotkey or paste. No required fields, no summary required at creation. Debt mechanism: a capture without a self-written summary appears in the open-loops panel and stays gray on the canvas. Promotion to permanent = choosing a type + writing your own summary/prose. The pasted original is retained (e.g. in an appendix block) after promotion.

## Storage layout

```
vault/
  template.typ          # shared typst defs: #meta, #l, styles
  templates/            # per-type note templates (editable notes)
  daily/  weekly/       # time-based
  permanent/            # all permanent types, flat; type is a #meta field
  capture/
  generated/
  .index/               # derived: SQLite (links, tags, positions, suggestions)
```

- Link syntax: `#l("note-id")` — valid typst, compiles standalone. Inserted via hotkey/autocomplete so it's cheap to type.
- Canvas positions and AI suggestions live in `.index/`, never in note files. Positions should survive index rebuilds (either a separate small positions file or excluded from "derived" purges).
- Index is rebuilt by parsing files; file watcher keeps it live.

## Editor

- **Hybrid block editing from v0**: the block containing the cursor shows raw typst source; all other blocks show rendered output. Per-block (not per-line) because typst constructs span lines and only compile as complete expressions.
- Rendering: embed the typst compiler (Rust crate) — compile note → SVG, cache per note, invalidate on edit. Fast enough at this scale.
- **Vim later, but architected for now**: keep a clean separation between the text buffer/edit-command layer and the widget layer, so a modal keymap can be inserted without rewriting the editor. No mature code-editor widget exists for Dioxus; the editor is built on primitives and is the highest-effort component of the project.
- Fallback if hybrid editing stalls: split view (source | rendered) is an acceptable temporary mode.

## Canvas (the table)

- **Persistent positions.** Every canvas note stores x/y. New notes auto-place near their strongest links; once moved by hand, position is permanent. Force-directed layout is at most an on-demand "arrange this cluster" command, never the default.
- **Semantic zoom.** Far: colored dots (color = type). Mid: title cards. Close: rendered typst body (cached SVGs). Viewport culling makes thousands of notes a non-issue for rendering.
- **Enter/exit nodes.** Clicking a card opens a modal editor over the canvas — edit, close, back to the table. The canvas stays visible around it to preserve the sense of place.
- **Findability**: filter by tag and by type (filtered-out cards dim, not disappear), plus jump-to-note search that pans/zooms to the card.
- **Edges**: real links (`#l(..)`) as solid edges; AI-suggested links as dashed edges (see below).
- Scale target: hundreds now, low thousands eventually.

## Friction system

One **open-loops panel**, always accessible, showing three kinds of debt:

1. Captures without a self-written summary (with age).
2. AI-suggested links neither written nor dismissed (with age).
3. Dangling `#l(..)` references to notes that don't exist.
4. Notes with missing or unparseable `#meta` (typeless notes) — load-bearing now that type lives only in metadata, not in the path.

Nothing blocks saving or writing. The debt is simply always visible and ages visibly. Additional soft incentives can be layered later (e.g. counts on the canvas), but the panel is the mechanism.

## AI integration

Two separate mechanisms:

1. **In-app**: the app shells out to the `claude` CLI headlessly for tagging and link suggestion over the vault.
2. **Vault-as-context**: an MCP server exposing the vault (search, read, link graph) so any Claude Code session can use the notes for projects, study, research.

### Suggested links — the core friction design

- Suggestions are stored in the sidecar index only. Discovery is free and ambient: dashed edges on the canvas, suggestion list per note.
- **Accepting = writing.** No accept button that edits the file for you. While editing a note, a relevant suggestion appears as gray ghost text at the cursor — press Tab to insert the `#l(..)` (code-completion style). You are still the one writing, in your own sentence, at a place you chose.
- Soft incentive (not requirement) to embed the link in a phrase saying *why* the connection exists — e.g. the ghost text nudges toward sentence context rather than bare link insertion. Exact mechanism TBD during implementation; must not add typing overhead beyond the Tab.
- Writing a suggested link (by Tab or manually) clears the suggestion; dismissing it clears it too. Un-actioned suggestions accumulate as visible debt.

### Tagging

AI proposes tags into the index; applying them to `#meta(..)` goes through the same explicit, low-cost confirm flow. (Tags are metadata, not prose, so a lighter touch than links is acceptable — but still never auto-applied.)

### Generated notes

Type exists from v1 (directory, canvas styling, model rules). Actual generation pipeline (e.g. digest a set of papers into a wiki) implemented later. Generated notes may be linked *from*, are visually marked, and are treated as disposable/regenerable — they are context, not knowledge.

## Roadmap

**v0 — daily driver for writing**
- Vault structure, `#meta` / `#l` conventions, shared template
- File CRUD, note creation from per-type templates, daily note with template + prev/next
- Hybrid block editor with live typst rendering (split-view fallback)
- Link index, backlinks panel, dangling-link detection
- Capture notes (global hotkey, paste) + open-loops panel (captures + dangling links)

**v1 — the table**
- Canvas with persistent positions, semantic zoom (dots → titles → rendered bodies)
- Modal open/edit/close on cards; tag & type filters; type colors; jump-to-note
- Auto-placement of new notes near linked ones; on-demand cluster arrange
- Capture/generated visual treatment; `generated` type defined

**v2 — vim**
- Modal editing layer on the existing buffer architecture, implemented incrementally as needed

**v3 — AI**
- `claude` CLI integration: tag proposals, link suggestions
- Dashed suggestion edges, ghost-text Tab-completion, suggestion debt in open-loops panel
- MCP server exposing the vault to Claude Code
- (later) generated-note pipeline

## Known risks

1. **The editor is half the project.** Hybrid block editing + typst rendering + (later) vim on Dioxus primitives, with no existing editor widget. Mitigation: split-view fallback, strict buffer/UI separation, per-block rather than per-character ambitions.
2. **Typst parsing for the index.** Extracting `#meta` and `#l` calls needs a real parse (the `typst-syntax` crate), not regex, to survive edge cases.
3. **Canvas position durability.** Positions are user data disguised as index data — must not be lost on index rebuilds.
4. **Scope creep at v1.** The canvas is where feature ideas multiply; the v1 list above is the ceiling, not the floor.
