# Phase 0 ships without undo; vim undo is born in phase 5

## Context

`roadmap-v2.md` line 39 said "Undo cannot lapse: the textarea's native undo dies with it, so a linear buffer undo/redo ships in the same phase, behind ordinary chords with palette entries; phase 5 re-cuts the grain to vim units and retires the chords."

## Decision

Taken with the user (2026-08-09): **no undo in phase 0**.
The textarea's native undo already died on every epoch remount (each link splice, each overlay close), so the loss is smaller than the roadmap priced.
Vim-grain undo (`u`, Ctrl+R, one insert session or one change = one step) is born directly in phase 5 — no stopgap chords to build, no palette entries to retire.

## Consequences

- `roadmap-v2.md` phase 0's undo item and exit line are amended; the exit criterion reads "selection and clipboard answer" without undo.
- Between phase 0 and phase 5, the editor has no undo at all. The daily-driving mitigation is the autosave plus git-style recovery the vault already affords; the user accepted the window.
- Phase 5 designs the history fresh (snapshot-based, explicit checkpoint), unconstrained by a phase-0 shape.

## Rejected

- **Snapshot undo behind Ctrl+Z now** — code built to be retired in phase 5, plus palette entries whose whole life is a deprecation; the user preferred the gap.
- **Splice-diff history** — finer memory for no phase-0 benefit; notes are small.
