# Vim friction batch: s S R, V highlight, surround, wrapped-line j/k

## Goal

The vim layer (`src/vim.rs`, `src/motions.rs`) grows the keys daily writing kept reaching for: `s`/`S`, `R` replace mode, a `V` highlight that covers whole lines, vim-surround's `cs` `ds` `ys` and visual `S`, and `j`/`k` that walk the wrapped lines the webview actually draws.

## Out of scope

- Macros, marks, named registers, visual block, the ex command line, the jumplist — the v2 ceiling stands (`docs/roadmap-v2.md` § Not in v2).
- Any change to insert mode's phase-0 flow, including its arrow keys (they keep walking logical lines).
- Operator-pending `j`/`k` (`dj`, `yk`, `cj`): they keep vim's linewise behaviour over logical lines.
- A count on `R`.
- Any new colour or spacing in `assets/theme.css` beyond what the V highlight strictly needs.

## Constraints

- The grammar stays pure over a `View` snapshot and headlessly testable; every new key gets its `acts_of`-style tests beside the existing ones (`src/vim.rs` § tests).
- Geometry the app does not own (wrapping) is read through an injected seam exactly like `ui::HitProbe` (`src/main.rs:95`): JS in `main`, a scripted fake in the headless tests.
- Every mutating key emits `Act::Checkpoint` once per change intent and records a semantic `Change` the dot replays (`adr/2026-08-undo-at-vim-grain.md`).
- The one register stays the clipboard (`adr/2026-08-one-register-the-clipboard.md`): `s`, `S`, `R`-overwrites, `ds`, `cs` route their cut text the way `c` and `d` do.
- No colour literal outside `assets/theme.css`; spacing in multiples of 4; `air` skill rules for the highlight change.
- No `while` loops; no naked `unwrap`; `expect` avoided in production code.
- Every decision taken lands as its own ADR under `docs/adr/`; `docs/roadmap-v2.md` records these as friction-earned additions to the polish backlog; `docs/plan.md` § Editor is updated where it lists keys.

## Items

1. `s` and `S` in normal mode (`src/vim.rs` `normal_character`): `s` cuts [count] clusters at the caret and enters insert (vim's `cl`), `S` changes the whole current lines (vim's `cc`); both checkpoint, set the clipboard, and record a `Change` the dot replays with the typed text.
2. `R` replace mode: a new `Mode` entered from normal, where each typed cluster overwrites the cluster under the caret (appending past the line's end), Backspace steps back and restores the overwritten cluster (past the session start it only moves), Escape returns to normal as insert's does; one checkpoint per session, dot-replayable, the box caret drawn as in normal mode.
3. The `V` highlight (`src/ui.rs` active-block widget, the selection drawing phase 0 introduced): in `Mode::Visual(VisualKind::Line)` the drawn selection covers every line from anchor to head over its text extent, first to last character plus a trailing newline cell, instead of the charwise anchor..head span; `v` keeps the charwise drawing.
4. Surround targets in `src/motions.rs`: the surround pair set `* _ ' " \` ( ) [ ] { } < >`, where `*` and `_` (Typst emphasis) pair left-to-right like the quote objects and brackets nest like the bracket objects; a function that, from the caret, finds the innermost enclosing pair of a given kind and returns both delimiter spans.
5. `cs<old><new>` and `ds<old>` in normal mode: two-key and one-key prefixes after `c` / `d` followed by `s`, resolving to one splice that replaces or removes the two delimiters; checkpointed, recorded for the dot; a missing target aborts quietly like the objects do (`a_verb_with_a_broken_noun_aborts`).
6. `ys<noun><pair>` and visual `S<pair>`: `y` then `s` arms a surround-pending state taking a motion or text object, then one pair key; visual `S<pair>` wraps the selection (char-wise the end clusters inclusive, line-wise the whole lines); an opening bracket pads with spaces (`( x )`), a closing one does not (`(x)`), every other pair is bare; the yank register is untouched.
7. A wrapped-line geometry seam beside `HitProbe` (`src/ui.rs`, `src/main.rs`): given a note-global byte and a goal x in pixels, the webview answers the byte one visual line up or down at that x (`caretPositionFromPoint` at the caret's rect ± its line height), with a headless fake for tests.
8. Plain `j`/`k` in normal and visual mode (with counts) walk visual lines through that seam: `Act`s the executor resolves asynchronously, the goal column becoming a pixel x held across a `j`/`k` run and forgotten by everything else; operator-pending `j`/`k` are untouched; the last visual line of the note and the first clamp as today.
9. Documentation and decision trail: one ADR per decision (surround pair set and padding rule, replace-mode backspace, visual-line j/k through a geometry seam, V highlight extent), `docs/roadmap-v2.md` polish backlog and `docs/plan.md` § Editor updated.

## Acceptance

- `make static && make test` green at 100% region/line/function coverage.
- Headless grammar tests cover every new key, its count, its checkpoint, its dot replay and its quiet failure.
- The user verifies in the app (`make run`) on a long wrapped paragraph that `j`/`k` step one drawn line at a time, and that `V` paints whole lines.

## Check

`make static && make test`
