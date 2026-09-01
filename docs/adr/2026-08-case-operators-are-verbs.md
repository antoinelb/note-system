# gu gU g~ join the verbs; Operator grows from three to six

## Context

`~` flipped one cluster and nothing else could change case over a span. The
operator machinery — counts, motions, objects, the dot, visual — already
existed; the case verbs only needed to be spelled as operators to inherit all
of it.

## Decision

- **`Operator` gains `Lower`, `Upper`, `Flip`.** `finish_operator` gets one arm
  for all three: checkpoint, splice the recased span, caret at the span's start
  (or left where it stood for a linewise span, the same rule `y` uses).
- **They fill no register.** `Operator::recasing()` guards the `SetClipboard`
  push. Nothing moved, so nothing was cut — and clobbering the clipboard,
  which is the one register
  (`adr/2026-08-one-register-the-clipboard.md`), would be a real loss for no
  gain.
- **Armed behind `g`**, and doubled two ways: `guu` (whose second key is not a
  verb key, so it cannot reach `operator_key`'s doubling rule and gets an
  explicit arm) and `gugu` (which comes back round through the `g` prefix and
  falls out of `case_key` for free). Both work; vim supports both.
- **Visual `u`, `U` and `~` are the same three verbs** through the existing
  `visual_operate`, so the selection path needed no new code at all.
- **`.` works untouched**: `repeat` already replays `Change::Operate { op, .. }`
  through the same resolution the live keystroke used.
- Normal-mode `~` keeps its own single-cluster behaviour and its own
  `Change::Toggle`; both it and `g~` now share one `recase` helper, so the
  Unicode case folding lives in one place.

## Rejected

- **A separate `CaseVerb` enum beside `Operator`** — every function taking an
  operator (`finish_operator`, `current_lines`, `resolve_noun`, `visual_operate`,
  the dot's `Change::Operate`) would have needed a second, parallel path.
- **Recasing through the clipboard** (cut, transform, paste) — three acts and a
  clobbered register for what is one splice.
- **`g?` (rot13)** — vim has it; nobody has ever wanted it.
