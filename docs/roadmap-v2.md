# v2 Roadmap — vim

Task breakdown of `plan.md` § Roadmap, v2.
The plan carries the *what* and *why*; this file carries the *order* and the *state*.
Ordering rationale: `adr/2026-08-v2-caret-first-order.md` (the caret leads).
The seam this version fills was architected in v0 phase 5 and has held since: "the v2 modal keymap slots in between the two without touching either" (`src/editor.rs`; `plan.md` § Editor, `adr/2026-07-buffer-is-path-plus-string.md`).
One editor serves every surface — the logs centre pane and the writing sheet mount the same machinery (`adr/2026-08-sheet-reuses-the-one-editor.md`) — so each phase below lands everywhere at once.
**"Implemented incrementally as needed" is the design, not a disclaimer** (`plan.md` § Roadmap): every phase leaves the editor daily-drivable, and the *not in v2* list at the bottom is the ceiling — a key earns its way in through daily friction, never through completeness.

**v2 is complete. Next → v3 (`plan.md` § Roadmap), or the polish backlogs.**

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
- [x] The Escape ladder: insert → normal → rendered block; the mode is editor-wide and survives slides and activations, and a new file starts thinking — normal at every note open (`adr/2026-08-escape-ladder-editor-wide-mode.md`).
- [x] Entering insert: i a I A o O (o and O open a line *inside* the block; a blank line splits it at the next resegmentation — the existing merge/split semantics, no new rules).
- [x] The caret is the mode indicator: bar in insert, box in normal (`adr/2026-08-caret-shape-is-the-mode-indicator.md`, with the palette boundary: modal keys take no palette entries; chords keep theirs and answer in both modes).
- [x] Unbound normal-mode keys are inert — swallowed, never inserted as text; a normal-mode composition is discarded whole.
- [x] Tests: every rung of the ladder, mode survival across slides, inert unbound keys, each insert entry places the caret where vim would.

Exit: Escape thinks instead of closing, i writes, and phase 0's writing flow is unchanged inside insert mode.

## Phase 2 — Motions

Goal: the note navigable at the speed of intent — a writing session never touches the arrow keys.

- [x] h j k l; w b e; 0 ^ $; f F t T with ; and ,; gg G — all with counts (`src/motions.rs`, `adr/2026-08-motions-on-visible-lines.md`).
- [x] Motions are note-scoped and blocks follow the caret: every landing goes through `Editor::place_at`, which wakes the owning block via the flush-and-resegment path; gg and G land on the first and last blocks.
- [x] j and k keep the goal column, falling through short lines without forgetting it.
- [x] Tests: each motion against fixture text, counts, boundary slides, goal-column memory, motion over multi-byte French text (char-wise, never byte-wise).

Exit: h j k l w b $ gg carry you anywhere in the note; the arrows are nostalgia.

## Phase 3 — Operators and text objects

Goal: editing becomes grammar — verb, count, noun.

- [x] Operators d c y over the phase-2 motions; the doubled line forms dd cc yy; the shorthands D C Y x X r ~; paste with p P; counts compose (d2w, 3dd, 2d3w); c ends in insert.
- [x] Text objects: iw aw, i" a" i' a' i` a`, i( a) and the sibling pairs — « » included — and ip ap have the house meaning: the paragraph *is* the block, read off the block map, not a scan.
- [x] One register, and it is the system clipboard; linewise-ness rides the trailing newline (`adr/2026-08-one-register-the-clipboard.md`); named registers wait for demonstrated need.
- [x] Cross-block changes go through `Editor::splice`: within-block spans ride the typing path, crossing spans splice-then-resegment (`adr/2026-08-editor-splice-cross-block.md`).
- [x] Tests: the operator × motion matrix on fixture text, objects at edges and under nesting, clipboard round-trip, cross-block changes resegment and save.

Exit: diw, ci", yy then p — no mouse selection survives in the writing flow.

## Phase 4 — Visual mode

Goal: see the span before choosing the verb.

- [x] v (character-wise) and V (line-wise): motions extend the selection, o swaps its ends, Escape returns to normal (`adr/2026-08-visual-selection-is-the-anchor.md`).
- [x] The phase-3 operators apply to the selection; the drawing is phase 0's selection highlight, riding the same anchor.
- [x] Tests: extension by each motion class, o, operator application, Escape restores normal with the caret where vim leaves it.

Exit: v e d reads like the sentence it is.

## Phase 5 — Repeat, vim undo, search

Goal: the finishing grammar — . and u and / are what make it vim rather than modal arrow keys.

- [x] Undo born at vim grain: one insert session or one change = one undo step, behind u and Ctrl+R — the editor's first undo, whole-note snapshots per change intent (`adr/2026-08-undo-at-vim-grain.md`).
- [x] `.` repeats the last change — operator applications, shorthands, pastes and insert sessions replay semantically; `[count].` overrides.
- [x] / search within the note with smartcase and wrap-around, n N to walk matches: the landing is `Editor::place_at`, so a hit in a rendered block activates it (`adr/2026-08-search-lands-through-place.md`).
- [x] Tests: undo grain over insert sessions and operators, dot after each change class, search landing across block boundaries.

Exit: u undoes what one intent did, . repeats it, / finds it.

## Not in v2 — the ceiling

Macros, marks, named registers, visual block, the ex command line (`:`, `:s`), the jumplist, any vim configuration surface.
Anything above earns its way in through daily friction, queued in the polish backlog first — "as needed" cuts both ways.

## v2 exit criteria (from `plan.md`)

- [x] The modal keymap sits between the widget and `Editor`, inserted without rewriting either — the invariant held from v0 phase 5 to its payoff (`plan.md` § Editor; `src/vim.rs` is the slot filled)
- [x] The owned-caret widget retired the textarea, and the v0 polish-backlog item with it; composition, clipboard and selection survived the move (phase 0; undo arrived with phase 5, `adr/2026-08-no-stopgap-undo-in-phase-0.md`)
- [x] Normal, insert and visual modes; note-scoped motions; operators and text objects; vim-grain undo, dot repeat, in-note search (phases 1–5)
- [x] `make test` green with 100% coverage; every note still compiles with vanilla typst (`make check-vault`)
- [x] The ceiling held: nothing shipped in v2 beyond this list
