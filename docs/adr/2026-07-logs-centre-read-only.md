# The logs centre pane is read-only until phase 8

## Context

Phase 7 deletes the phase-5 split-view widget — the design's centre column
has no room for two panes.
The roadmap sketched "the buffer moves into the centre pane", which implied
shipping the source ⇄ rendered toggle (phase 8's pre-declared fallback)
early.

## Decision

The phase-7 centre pane renders the selected note as SVG and offers **no
editing**.
Writing returns with phase 8 (hybrid block editor, or its declared
source ⇄ rendered fallback) on the untouched buffer layer in `editor.rs`.

Everything that existed only to serve in-app writing leaves `ui.rs` with the
split view: the debounced-autosave resource, `apply_edit` wiring, and the
Ctrl+Q flush registry.
**Ctrl+Q now closes directly** — with no buffer ever open there is nothing
to flush, so this supersedes `2026-07-ctrl-q-flushes-then-closes.md` until
editing returns (phase 8 must reinstate a flush-then-close path along with
the buffer).

## Rejected

- **Source ⇄ rendered toggle now**: the fallback shape arriving one phase
  early. Rejected to keep phase 7 purely about navigation; the toggle's
  natural home is phase 8, where block segmentation either replaces or
  confirms it.
