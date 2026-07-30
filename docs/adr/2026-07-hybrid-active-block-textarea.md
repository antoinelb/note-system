# The hybrid editor mounts a textarea on the active block only

## Context

Phase 8 builds the hybrid block editor: the block under the cursor shows raw
typst source, every other block shows rendered output.
`adr/2026-07-buffer-is-path-plus-string.md` deferred cursor and edit
operations to this phase and sketched them as buffer-owned
(`insert_char`/`delete_back`/`move_cursor`, "by then the widget is ours
rather than the browser's").

## Decision

- The active block's rendered SVG is replaced by a **`<textarea>`** showing
  that block's source; every keystroke stays native to the webview.
  (Wireframe 1b's alert-colour border was dropped on first use: the warm
  element is the caret, not a frame —
  `adr/2026-07-note-rendering-theme-input.md`.)
  The buffer's one edit operation is `Buffer::replace_range(span, text)` —
  the widget hands back the whole block value, the buffer splices it into
  the note. Stale or non-boundary spans drop the edit instead of panicking.
- The browser owns the in-block caret. This keeps French dead-key
  composition, clipboard, selection and key repeat for free — hand-rolling
  those was the real cost of the buffer-owned-cursor sketch, and the strict
  buffer/widget separation survives unchanged: a v2 vim layer replaces the
  widget and calls the same `replace_range`.
- **Block switching**: clicking a rendered block activates it; ArrowUp on
  the textarea's first line / ArrowDown on its last line slides to the
  adjacent block; Escape deactivates back to rendered. The caret position
  needed for the boundary test comes from a small JS `selectionStart` probe
  injected through root context (the `Closer` pattern), so headless tests
  fake it. At those edges the browser default is a no-op, so the async probe
  needs no preventDefault.
- `apply_edit` and `set_text` are deleted — nothing calls them once edits go
  through `replace_range`.
- Editing returns to the centre pane, ending the read-only suspension of
  `adr/2026-07-logs-centre-read-only.md`: the debounced autosave
  (`adr/2026-07-debounced-autosave.md`) and the Ctrl+Q flush-then-close path
  (`adr/2026-07-ctrl-q-flushes-then-closes.md`) are reinstated.

## Rejected

- **Buffer-owned cursor and per-key edit ops now** — vim-shaped from day
  one, but it reimplements caret drawing, selection, clipboard and IME
  composition inside `onkeydown`, and dead keys (`^` + `e` → `ê`) only
  surface through composition events. All of it is free in a textarea, and
  none of it is needed until vim's normal mode exists.
- **contenteditable** — the same caret problems plus HTML sanitisation on
  top; strictly worse than a textarea for plain source.
- **The pre-declared fallback (source ⇄ rendered toggle)** — abandons the
  hybrid goal while the real thing is affordable; kept only as the retreat
  if this stalls (`plan.md` § Known risks).
