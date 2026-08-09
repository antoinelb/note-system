# v2 orders itself caret-first: the widget before the modes

## Context

v2 is the vim layer the editor was architected for since v0 phase 5: the widget forwards events to `Editor`, and "the v2 modal keymap slots in between the two without touching either" (`plan.md` § Editor, `adr/2026-07-buffer-is-path-plus-string.md`).
The active block today is a native `<textarea>` (`adr/2026-07-hybrid-active-block-textarea.md`), which bought composition, clipboard, selection and key repeat for free — at the price that the browser owns the caret.
WebKitGTK has no `caret-shape`, so that caret is a bar forever, and normal mode's box caret is the mode indicator in a design whose chrome refuses labels.
The v0 polish backlog already named the homemade widget as v2 groundwork.

## Decision

`roadmap-v2.md` orders v2 as: owned-caret widget → modes → motions → operators and objects → visual → repeat/undo/search.

- The widget goes first, behaviour-neutral, absorbing the textarea's free features (selection, clipboard, key repeat, dead-key composition) and a stopgap linear undo before any mode exists — every later phase then works against app-owned caret state, headlessly testable, and the `CaretProbe`/`CaretWriter` seams retire.
- Each later phase is one self-sufficient editing gain, daily-driven before the next — "implemented incrementally as needed" made structural.
- A *not in v2* list (macros, marks, named registers, visual block, the ex line, the jumplist, configuration) closes the version the way v1's ceiling did.

## Rejected

- **Modes on the native textarea first** — normal mode behind a bar caret is invisible, every normal-mode key fights the textarea's own editing through preventDefault, and the caret would have two owners syncing through JS eval: the exact bug factory the phase-8 ADR avoided by giving the browser the whole caret or none of it.
- **A modal-editing dependency** (a vim-emulation crate) — the seam makes the layer cheap to write, the app is extended by editing the source, and block-boundary behaviour — the one part of this editor vim has no answer for — would be dictated by someone else's grammar.
- **Big-bang vim** — weeks without a daily-drivable editor; the roadmap loop is per-phase exits, and phase 0's behaviour-neutrality makes the retreat free if the widget stalls.
