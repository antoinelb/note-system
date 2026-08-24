# Visual mode rides phase 0's anchor

## Context

v2 phase 4: see the span before choosing the verb.

## Decision

- **The selection *is* the phase-0 anchor on `Editor`** — the field Shift-arrows and mouse drags already use, drawn by the same `Selected` pieces. Visual mode adds zero drawing and zero selection state: `v`/`V` just switch the grammar's mode over the collapsed caret, motions emit `Extend` (head moves, anchor holds) instead of `Place`, and `o` is `swap_ends`.
- **The arrows extend in visual** — collapsing mid-selection is the one thing no one means — and the whole motion vocabulary answers: counts, `f/t` prefixes, `gg/G`.
- **Operators close the mode**: char-wise takes both end clusters (vim's inclusive visual), line-wise takes whole lines through `motions::linewise_span`; `x` is `d`; `c` falls into insert; Escape returns to normal with the caret at the head. Same kind toggles out; the other kind switches in place.
- **Selection may cross blocks**: the operator applies to the true note-global span through `Editor::splice`; the *drawing* clips to the active block, since only it renders source. A line-wise selection draws its raw ends, not full-line highlights — both noted as friction-backlog candidates, not blockers.

## Rejected

- **Separate visual-selection state in `Vim`** — a second highlight and a second clamping policy for the same pixels.
- **`p` over a selection, `viw`-style objects in visual** — the roadmap's phase-4 list is motions + operators; the rest earns its way in.
