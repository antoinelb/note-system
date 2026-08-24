# Undo is born at vim grain: snapshots per change intent

## Context

Phase 0 shipped without undo by decision (`adr/2026-08-no-stopgap-undo-in-phase-0.md`); phase 5 is where the editor's first undo arrives, already at vim grain.

## Decision

- **Whole-note snapshots on `Editor`** (`{text, head}`), one per *change intent*, never per keystroke: the grammar emits `Act::Checkpoint` before every mutating resolution — an operator's splice, `x`, `r`, `~`, a paste — and once on entering insert, so one insert session or one `d2w` is one `u`.
- The opened file seeds the history, so even a session that never leaves insert can fall back to it. Consecutive identical snapshots dedup, and `u` steps over checkpoints equal to the present — an entered-then-abandoned session costs no press. Depth caps at 100; the history dies with its editor, per-note.
- Yank checkpoints nothing — it changes nothing.
- **Keys**: `u` in normal; `Ctrl+R` is the grammar's one ctrl carve-out (no palette chord uses it; the palette boundary otherwise holds). Restoring resegments and wakes the caret's block — the splice mechanism.
- **The dot** records the last change *semantically* — verb + noun + count, `x`/`r`/`~`/paste parameters, or an insert entry — plus the session's typed text captured at Escape (`text[session start..head]`, empty if the caret wandered backward). `.` re-resolves at the caret it finds; `[count].` overrides the recorded count; a change-verb replay applies its recorded text and never reopens insert.

## Rejected

- **Splice-diff history** — finer memory for no benefit at note scale; snapshots are trivially correct under resegmentation.
- **Keystroke recording for the dot** — replaying keys through the grammar reintroduces every mode edge; the semantic record replays through the same resolution code the original used.
- **Visual-operator repeats** — vim repeats them over a same-sized region; deferred to the friction backlog with the rest of the visual extras.
