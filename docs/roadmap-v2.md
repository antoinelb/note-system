# v2 Roadmap — vim

Task breakdown of `plan.md` § Roadmap, v2.
The plan carries the *what* and *why*; this file carries the *order* and the *state*.
Ordering rationale: `adr/2026-08-v2-caret-first-order.md` (the caret leads).
The seam this version fills was architected in v0 phase 5 and has held since: "the v2 modal keymap slots in between the two without touching either" (`src/editor.rs`; `plan.md` § Editor, `adr/2026-07-buffer-is-path-plus-string.md`).
One editor serves every surface — the logs centre pane and the writing sheet mount the same machinery (`adr/2026-08-sheet-reuses-the-one-editor.md`) — so each phase below lands everywhere at once.
**"Implemented incrementally as needed" is the design, not a disclaimer** (`plan.md` § Roadmap): every phase leaves the editor daily-drivable, and the *not in v2* list at the bottom is the ceiling — a key earns its way in through daily friction, never through completeness.

**Next step → phase 2; or the v0/v1 polish backlogs.**

## How we work

The v0 loop, unchanged (`roadmap-v0.md` § How we work):

- The only goal is building the note system (`adr/2026-07-goal-build-only.md`).
- Items marked **→ ADR** are decisions to take together at the start of the task, then record in `docs/adr/`.
- A phase is done when its exit criterion holds and `make test` passes with 100% coverage; a shortfall on regions is diagnosed per instantiation before writing any test (`adr/2026-07-coverage-100-percent-lines.md`).
- Per-task loop: discuss approach → decisions become ADRs → implement with tests until green → commit.
- Read `.claude/dioxus.md` before any Dioxus code (every time).
- The palette invariant holds where it applies: a phase that adds a *chord* adds its palette entry in the same change; modal keys are a grammar, not commands, and take none (phase 1 records the boundary).

## Phase 0 — The owned caret

Goal: the active block's caret is drawn by the app — the textarea retires, and typing feels exactly as it did.

This is the v0 polish-backlog item pulled to the front as v2's foundation: WebKitGTK has no `caret-shape`, so a native textarea can never show normal mode's box caret (`roadmap-v0.md` § Polish backlog, `adr/2026-07-hybrid-active-block-textarea.md`).
The phase is deliberately behaviour-neutral — no modes yet — so the pre-declared retreat is free: if the widget stalls, the textarea stays and v2 waits.
This is the editor's second iceberg (`plan.md` § Known risks 1 still applies); composition is the part to budget for.

- [x] A homemade active-block widget: the block's source with an app-drawn caret capable of bar *and* box, mounted exactly where the textarea was; the widget still hands whole-block values to `Editor::edit`, and `Buffer` stays a path and a `String`.
  - [x] Where the caret lives: on `Editor`, in note-global bytes (`adr/2026-08-caret-on-editor-note-bytes.md`); the pure text math and render model live in `src/caret.rs`.
- [x] The textarea's free features, hand-rolled — the exact costs `adr/2026-07-hybrid-active-block-textarea.md` priced in when it deferred them:
  - [x] key handling and key repeat;
  - [x] selection: Shift-arrows, mouse drag, a drawn highlight (phase 4's visual mode will reuse the drawing);
  - [x] clipboard: Ctrl+C/X/V through injected seams — the `navigator.clipboard` pattern capture already uses, plus a write seam;
  - [x] **French dead-key composition** (`^` + `e` → `ê`) via composition events on a hidden IME sink (`adr/2026-08-hidden-ime-sink.md`) — spike-verified on WebKitGTK before implementation.
- [x] The caret seams dissolve: `CaretProbe` and `CaretWriter` retired with their staleness rules — slides, the link picker and every overlay read and write app state directly; a `HitProbe` seam answers mouse geometry, which stays the webview's.
- [x] ~~Undo cannot lapse~~ **Phase 0 ships without undo** (`adr/2026-08-no-stopgap-undo-in-phase-0.md`): the textarea's native undo already died on every remount, so the stopgap was cut with the user; vim-grain undo is born in phase 5.
- [x] Tests: caret movement and drawing state, selection maths, composition sequences, clipboard through the seams, typing round-trips byte-identical to the textarea era.

Exit: days of daily writing in the homemade widget are indistinguishable from the textarea — dead keys compose, repeat repeats, selection and clipboard answer (undo waits for phase 5 by decision).

## Phase 1 — Modes

Goal: normal and insert exist, and the caret announces which one you are in.

- [x] `Mode` lives in the keymap layer between the widget and `Editor` — the slot `editor.rs` names (`src/vim.rs`); the widget forwards keys, the layer translates them into `Editor` calls or swallows them.
- [x] The Escape ladder: insert → normal → rendered block; the mode is editor-wide and survives slides and activations, and the editor opens writing — insert at launch (`adr/2026-08-escape-ladder-editor-wide-mode.md`).
- [x] Entering insert: i a I A o O (o and O open a line *inside* the block; a blank line splits it at the next resegmentation — the existing merge/split semantics, no new rules).
- [x] The caret is the mode indicator: bar in insert, box in normal (`adr/2026-08-caret-shape-is-the-mode-indicator.md`, with the palette boundary: modal keys take no palette entries; chords keep theirs and answer in both modes).
- [x] Unbound normal-mode keys are inert — swallowed, never inserted as text; a normal-mode composition is discarded whole.
- [x] Tests: every rung of the ladder, mode survival across slides, inert unbound keys, each insert entry places the caret where vim would.

Exit: Escape thinks instead of closing, i writes, and phase 0's writing flow is unchanged inside insert mode.

## Phase 2 — Motions

Goal: the note navigable at the speed of intent — a writing session never touches the arrow keys.

- [ ] h j k l; w b e; 0 ^ $; f F t T with ; and ,; gg G — all with counts.
- [ ] Motions are note-scoped and blocks follow the caret: j from a block's last line slides to the next block through the flush-and-resegment path the boundary arrows already use (`Editor::slide`); gg and G land on the first and last blocks.
- [ ] j and k keep the goal column, falling through short lines without forgetting it.
- [ ] Tests: each motion against fixture text, counts, boundary slides, goal-column memory, motion over multi-byte French text (char-wise, never byte-wise).

Exit: h j k l w b $ gg carry you anywhere in the note; the arrows are nostalgia.

## Phase 3 — Operators and text objects

Goal: editing becomes grammar — verb, count, noun.

- [ ] Operators d c y over the phase-2 motions; the doubled line forms dd cc yy; the shorthands D C Y x X r ~; paste with p P; counts compose (d2w, 3dd); c ends in insert.
- [ ] Text objects: iw aw, i" a", i( a) and the sibling pairs — and ip ap have a house meaning: the paragraph *is* the block, so the object comes from the block map, not a scan.
- [ ] One register, and it is the system clipboard (vim's `clipboard=unnamedplus` as the only behaviour): y and p interop with the OS through the phase-0 seam; named registers wait for demonstrated need. **→ ADR**
- [ ] A change whose span crosses the active block (dG, dj on a block's last line): `Buffer::replace_range` already spans the whole note — the mechanism is splice then resegment, as deactivation does; the decision is the shape of the `Editor` entry point beside block-scoped `edit`. **→ ADR**
- [ ] Tests: the operator × motion matrix on fixture text, objects at edges and under nesting, clipboard round-trip, cross-block changes resegment and save.

Exit: diw, ci", yy then p — no mouse selection survives in the writing flow.

## Phase 4 — Visual mode

Goal: see the span before choosing the verb.

- [ ] v (character-wise) and V (line-wise): motions extend the selection, o swaps its ends, Escape returns to normal.
- [ ] The phase-3 operators apply to the selection; the drawing is phase 0's selection highlight.
- [ ] Tests: extension by each motion class, o, operator application, Escape restores normal with the caret where vim leaves it.

Exit: v e d reads like the sentence it is.

## Phase 5 — Repeat, vim undo, search

Goal: the finishing grammar — . and u and / are what make it vim rather than modal arrow keys.

- [ ] Undo born at vim grain: one insert session or one change = one undo step, behind u and Ctrl+R — the editor's first undo, phase 0 having shipped without one (`adr/2026-08-no-stopgap-undo-in-phase-0.md`). **→ ADR**
- [ ] `.` repeats the last change — operator applications and insert sessions replay.
- [ ] / search within the note, n N to walk matches: a match may live in a rendered block, so landing activates it and places the caret — the offset-walking cousin of Ctrl+Enter's `links::link_at`.
- [ ] Tests: undo grain over insert sessions and operators, dot after each change class, search landing across block boundaries.

Exit: u undoes what one intent did, . repeats it, / finds it.

## Not in v2 — the ceiling

Macros, marks, named registers, visual block, the ex command line (`:`, `:s`), the jumplist, any vim configuration surface.
Anything above earns its way in through daily friction, queued in the polish backlog first — "as needed" cuts both ways.

## v2 exit criteria (from `plan.md`)

- [ ] The modal keymap sits between the widget and `Editor`, inserted without rewriting either — the invariant held from v0 phase 5 to its payoff (`plan.md` § Editor)
- [ ] The owned-caret widget retired the textarea, and the v0 polish-backlog item with it; composition, clipboard and selection survived the move (phase 0; undo arrives with phase 5, `adr/2026-08-no-stopgap-undo-in-phase-0.md`)
- [ ] Normal, insert and visual modes; note-scoped motions; operators and text objects; vim-grain undo, dot repeat, in-note search (phases 1–5)
- [ ] `make test` green with 100% coverage; every note still compiles with vanilla typst (`make check-vault`)
- [ ] The ceiling held: nothing shipped in v2 beyond this list
