# v3 Roadmap — AI

Task breakdown of `plan.md` § Roadmap, v3.
The plan carries the *what* and *why*; this file carries the *order* and the *state*.
Ordering rationale: `adr/2026-08-v3-mcp-first-order.md` (the vault answers before it suggests).
The governing invariants, restated because every phase below leans on them: **AI never writes prose in note files**; **accepting = writing** — Tab-inserted ghost text, never an accept button that edits the file for you; **no hard blocks** — suggestion debt is visible in the open loops, never a save-blocker (`plan.md` § Design principles, § AI integration).
Two decisions taken with the roadmap frame everything below: `claude` runs only when asked (`adr/2026-08-suggestions-manual-trigger.md`), and suggestions with their dismissals survive index rebuilds in their own store (`adr/2026-08-suggestions-own-durable-store.md`).
Phase 4 rides v2 phase 0 — ghost text is drawn by the owned-caret widget, which the textarea cannot do — and is the one phase that waits if v3 somehow opens first.

**Next step → v2 comes first (`roadmap-v2.md`); when v3 opens, phase 0 below.**

## How we work

The v0 loop, unchanged (`roadmap-v0.md` § How we work):

- The only goal is building the note system (`adr/2026-07-goal-build-only.md`).
- Items marked **→ ADR** are decisions to take together at the start of the task, then record in `docs/adr/`.
- A phase is done when its exit criterion holds and `make test` passes with 100% coverage; a shortfall on regions is diagnosed per instantiation before writing any test (`adr/2026-07-coverage-100-percent-lines.md`).
- Per-task loop: discuss approach → decisions become ADRs → implement with tests until green → commit.
- Read `.claude/dioxus.md` before any Dioxus code (every time).
- The palette invariant holds: a phase that adds a chord adds its palette entry in the same change; Tab joins the modal keys on the other side of the boundary v2 phase 1 records — editor-local, no entry.

## Phase 0 — The vault answers (MCP)

Goal: any Claude Code session can search the vault, read a note, and walk the link graph — before the app itself gets any smarter.

- [ ] `app --mcp`: a second-process arm in `main.rs` exactly like `--capture`'s, with all logic in a new module so main stays plumbing (`adr/2026-08-capture-headless-second-process.md`); the process serves stdio and never opens a window.
- [ ] The MCP crate — and with it the repo's first serialization dependency (suggestion: the official Rust MCP SDK; the alternative, hand-rolled JSON-RPC, trades a dependency for a protocol re-implementation). **→ ADR**
- [ ] The read-only tool surface: search (title/tag/content), read a note, links of a note (outgoing and backlinks), list by type or category — and whether the server reads the SQLite index or the files themselves (plain files are the source of truth, and the index may be mid-write by the running app). **→ ADR**
- [ ] The server never writes: no create, no edit, no delete — the AI-never-writes-prose invariant enforced at the transport, not by convention.
- [ ] Registration documented: the `claude mcp add` / `.mcp.json` line that makes any project's session vault-aware.
- [ ] Tests: every tool's logic as pure functions against the fixture vault; malformed requests answer with errors, never panics.

Exit: from a Claude Code session in another project, ask a question only the vault can answer — and it does.

## Phase 1 — The suggestion store

Goal: suggestions and dismissals exist as durable data with a debt line, before any AI writes into them.

- [ ] A suggestion is (source note, target id, and whatever the engine turns out to need); the record shape is decided with the store. **→ ADR**
- [ ] The store lives beside positions, outside the disposable index — suggestions cost `claude` calls and dismissals have no upstream at all; neither survives `index.rebuild()` today, and both must (`adr/2026-08-suggestions-own-durable-store.md`); the concrete shape is the task's first decision (plain-lines file like `positions.rs`, or a second SQLite file). **→ ADR**
- [ ] A dismissed pair is remembered forever: never re-proposed by the engine, never resurfaced by a rebuild.
- [ ] Suggestions become the fourth debt kind: a store-side query, a fourth parameter chained in `loops::lines`, threading through `ui.rs::open_loops` — the ember, the list and the count follow automatically, and the chrome cannot disagree with the list.
- [ ] A suggestion whose link now exists (written by hand, and later by Tab) self-clears on re-index: the files stay the authority on what is *linked*; the store holds only what is *proposed* and what is *dismissed*.
- [ ] Tests: store round-trip, survival across an app-start rebuild, dismissal permanence, self-clear on a written link, the loops line format.

Exit: a hand-inserted suggestion row shows in the open-loops list, survives a restart, and disappears the moment the link is written by hand.

## Phase 2 — The engine (`claude` headless)

Goal: a palette command fills the store with real suggestions for the note you are in.

- [ ] Shell out to `claude -p` with structured output, off the UI thread — tokio currently lacks the `process` feature, so the seam is `std::process::Command` on a thread or the feature is added; part of the same decision. **→ ADR**
- [ ] Prompt assembly is a pure function: the open note's body plus the candidate targets (ids and titles from the index); the reply is parsed strictly — malformed output is a logged failure with a UI notice, never a partial write into the store.
- [ ] Proposals are filtered before they land: never an existing link, never a dismissed pair, never a dangling target, never the note itself.
- [ ] The palette command (suggestion: "Suggest links · this note" only; a vault-wide sweep waits for demonstrated need) — `CommandId` variant, `COMMANDS` entry, dispatch arm and the chord test in the same change, per the palette invariant. **→ ADR** (command scope)
- [ ] Failure modes are first-class: `claude` missing from PATH, timeout, non-zero exit — each surfaces as a notice naming the reason, and the store is untouched.
- [ ] Tests: prompt assembly, reply parsing (well-formed, malformed, empty), every filter, every failure mode, command wiring.

Exit: run the command on a real note; plausible proposals appear as debt in the loops list, and running it twice proposes nothing new.

## Phase 3 — Ambient discovery (the surfaces)

Goal: suggestions are visible where you think — the table and the page — and never interrupt.

- [ ] Dashed edges with a hollow star on the table: a second loop over `table::edges` geometry in the `svg.edges` layer under the cards, a dash class beside the solid lines, colours only in `theme.css`, both themes filled in together.
- [ ] The end-of-page line on a rendered note, both screens (logs centre pane and writing sheet): "proposed · evergreen-notes → this note", carrying the app's only visible hint, "enter accept · x dismiss" — a sibling of the links-footer rows, pure logic in its own module, wiring beside `link_footer`.
- [ ] What enter does, exactly: accepting = writing, so enter cannot edit the file — it routes into writing (suggestion: activate the note in edit with the suggestion primed for phase 4's ghost text; until phase 4 lands, enter simply focuses the editor). **→ ADR**
- [ ] x dismisses: recorded in the durable store, and the edge, the page line and the debt line all clear from that one source.
- [ ] Suggestion edges respect Ctrl+F dimming like real edges; auto-placement and cluster arrange keep reading only real links (suggestion — a proposed connection should not move furniture). **→ ADR**
- [ ] Tests: dashed geometry beside solid, the footer line on both screens, a dismissal round-trip clearing every surface, dimming behaviour.

Exit: a suggestion is visible as a dashed edge and an end-of-page line; x makes it gone, everywhere, forever.

## Phase 4 — Ghost text (accepting = writing)

Goal: while writing, a relevant suggestion appears as gray ghost text at the caret; Tab lands the `#l(..)` in your own sentence.

- [ ] **Rides v2 phase 0**: the ghost is drawn by the owned-caret widget after the caret — the uncontrolled textarea cannot host it; if v3 runs first, this phase waits and phase 3's enter-to-edit stands alone.
- [ ] When the open note carries an unresolved suggestion and the caret sits in text, the ghost renders; Tab inserts through `Editor::insert` — the Ctrl+L picker's exact path (`links::format_link`) — and the suggestion clears through the store like any written link.
- [ ] Tab is editor-local, like the modal keys: no palette entry — the boundary v2 phase 1 records extends to it.
- [ ] The soft nudge toward sentence context — `plan.md` § AI integration: embed the link in a phrase saying *why*, at no typing cost beyond the Tab; the mechanism is decided at the task. **→ ADR**
- [ ] Typing past the ghost hides it without recording a dismissal — display and dismissal are different acts, and only x dismisses; the exact appearance rules travel with this decision. **→ ADR**
- [ ] Tests: appearance rules, a Tab round-trip byte-identical to a hand-written link, clearing, no ghost when nothing is relevant, multi-byte French text.

Exit: mid-sentence, Tab lands the link, and the dashed edge on the table turns solid.

## Phase 5 — Tags

Goal: AI proposes tags; confirming one writes it into `#meta` — the app's first machinery that edits metadata.

- [ ] Proposals ride the phase-2 engine (the same command or a sibling — decided at the task) into the same durable store, behind the same filters (existing tags, dismissed proposals). **→ ADR**
- [ ] The confirm flow is lighter than links — tags are metadata, not prose — but never auto-applied; where proposals surface and what confirms them (suggestion: at the note's meta line, enter/x like the page line). **→ ADR**
- [ ] The meta splice, the first-ever `#meta` rewriting: a text-level splice into the `tags: (..)` array using `typst-syntax` spans, never regex — through `Buffer::replace_range` for an open note, `std::fs::write` for a closed one, and the watcher re-indexes for free; malformed or anomalous meta refuses the splice with a notice, no repair attempts. **→ ADR**
- [ ] Tests: splice correctness (empty array, existing tags, malformed-meta refusal), confirm and dismiss flows, proposal clearing, and the spliced files still compile.

Exit: confirm a proposed tag and the `.typ` file carries it — written by machinery you invoked, in a file that still compiles.

## Not in v3 — the ceiling

The generated-note pipeline (`plan.md` keeps it "later"), background or scheduled suggestion runs, vault-wide sweeps beyond what phase 2 admits, MCP write tools, embeddings or local models, suggestion ranking or scoring UI, auto-applied anything.
Anything above earns its way in through daily friction, queued in the polish backlog first — "as needed" cuts both ways.

## v3 exit criteria (from `plan.md`)

- [ ] An MCP server exposes the vault, read-only, to any Claude Code session (phase 0)
- [ ] `claude` proposes links and tags on demand; suggestions and dismissals survive index rebuilds; a dismissed pair never returns (phases 1–2, 5)
- [ ] Dashed suggestion edges, the end-of-page proposed line, ghost-text Tab-completion, suggestion debt in the open-loops panel (phases 3–4)
- [ ] AI wrote prose in no note file; the only file edits AI proposals ever cause are user-invoked — Tab and the tag confirm
- [ ] `make test` green with 100% coverage; every note still compiles with vanilla typst (`make check-vault`)
- [ ] The ceiling held: nothing shipped in v3 beyond this list
