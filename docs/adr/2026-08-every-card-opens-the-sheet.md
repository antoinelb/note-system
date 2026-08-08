# Every card opens the sheet

## Context

The phase-3 goal line says "permanent notes become editable in-app", but
capture and generated cards sit on the same table.
Phase 4 then declares "capture promotion is editing, not a feature" — set
`type` in `#meta`, write the summary — and the sheet is the only editor the
table has.
Decided with the user at phase start.

## Decision

**All three kinds open a sheet on click — permanent, capture and
generated.**
`Editor::open` takes any path, `index.path_for_id` resolves any id, and the
open/close, autosave and flush machinery is kind-blind, so the wide scope is
the empty-diff option.
Capture promotion (phase 4) already requires captures editable here;
excluding generated notes would add a refusal branch and its test for a
kind nothing can even create by hand yet.

## Rejected

- **Permanent only (the goal line's letter)** — phase 4 would immediately
  re-open the decision for captures, and the refusal branch for the other
  kinds is pure added code.
- **Permanent + capture, generated stays inert** — defensible ("disposable
  outputs aren't for editing"), but the model already says generated notes
  are linkable and deletable, and an inert card on a clickable table is a
  surprise with no payoff.
