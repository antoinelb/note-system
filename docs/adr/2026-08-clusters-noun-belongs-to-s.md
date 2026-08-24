# `Noun::Clusters` belongs to `s`, and `S` is `cc` reused

## Context

The vim friction batch item 1 adds `s` and `S` to normal mode.
Vim itself spells both as compositions of existing verbs — `s` is `cl`, `S` is `cc` — and `src/vim.rs` already funnels every operator application through `finish_operator`, which owns checkpointing, clipboard fill, dot-recording and the change-verb's insert handoff.

## Decision

- **`s` is `Operator::Change` over a new noun, `Noun::Clusters`**: [count] clusters from the caret, clamped to the current line's end exactly as `x`'s `cut_clusters` already clamps.
  `change_clusters` records `Change::Operate { op: Change, noun: Noun::Clusters, .. }` and calls the same `finish_operator` every other change verb calls, so checkpointing, clipboard fill and the insert-session handoff (`c` ends in insert) come free and cannot drift from `cw`/`ciw`/etc.
- **`Noun::Clusters` is its own `Noun` variant, not `Noun::Motion(Motion::Right)`**: `Motion::Right`'s landing clamps at the line's *last cluster*, not at `line.end` — standing on that cluster, the motion resolves to itself and names an empty span, so the insert session would open in front of the cluster instead of eating it.
  `change_clusters` clamps at `line.end` instead, which only `Noun::Clusters` can spell.
  The dot still re-resolves the noun through `resolve_noun` on replay, reclamping to the line's end at replay time — the count is what is recorded, not a byte offset, so a `5s` that cut one cluster near a short line's end can eat all five on the dot against a longer line, matching vim.
- **`S` is `current_lines(Operator::Change, view)`** — the same `Noun::Lines` path `cc`/`dd`/`yy` already share. There is no `S`-specific span logic to add; it is `cc`, spelled as itself at the call site for readability.
- **`Noun::Clusters` belongs to `s` alone**: a caret-relative cluster count has no surround meaning — `ys` always names a motion, object or line span — so `ys`'s wrap grammar resolves to `WrapNoun`, a separate type with no `Clusters` variant.
  There is nothing left to refuse: `Noun::Clusters` is unrepresentable in a `WrapNoun`, by construction, both in the live grammar and in its dot replay (`repeat_wrap`).

## Rejected

- **A special-cased splice bypassing the operator grammar** — would need its own checkpoint, clipboard and dot-record bookkeeping, duplicating what `finish_operator` already gets right for every other change verb.
- **A dedicated function for `S`** — `current_lines` already takes the operator as a parameter; `cc` and `S` are the same call with nothing left to differ.
