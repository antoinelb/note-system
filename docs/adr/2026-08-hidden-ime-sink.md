# A hidden IME sink is the widget's keyboard socket

## Context

The owned-caret widget draws all text, caret and selection itself, but French dead-key composition (`^` + `e` → `ê`) is load-bearing and WebKitGTK attaches its input-method context only to editable elements — composition events do not fire on a plain focusable div.

A spike (scratch dioxus-desktop app, xdotool-driven on the real WebKitGTK) confirmed:

- composition events **do** fire on an invisible 1px, opacity-0 `input`;
- the commit keystroke's keydown arrives flagged `isComposing`;
- WebKitGTK fires an **empty compositionend before the real one**, with a stray unflagged keydown between them;
- `prevent_default()` on printable keydowns keeps the sink empty without disturbing composition;
- `caretPositionFromPoint` and `caretRangeFromPoint` both exist (the mouse probe's two spellings).

## Decision

The widget contains an invisible `input.ime-sink` — the CodeMirror/Monaco technique. It renders nothing and owns no visible caret; it exists so the IME has an editable element to compose into. "The textarea retires" means retires as *rendering and caret owner*.

- The sink's `onkeydown` forwards to `keymap::action`; `Some` means prevent default, stop propagation, apply — `None` bubbles exactly as the textarea let keys bubble (Escape to the pane, every app chord to its handler).
- A keydown is never touched when `isComposing` is set, the key is `Dead`, **or a composition is open** — the last guard is what absorbs the ordering surprises the spike saw. (It was first written as "a composition *preview* is open", which made it insert-only and let the stray keydown through everywhere else: `adr/2026-08-composition-ownership-is-mode-independent.md`.)
- `compositionupdate` drives a preview signal drawn at the caret (`Piece::Preview`); only a non-empty `compositionend` reaches the buffer, so the early empty end commits nothing. (Outside insert mode a single-cluster commit now reaches the *grammar* instead of being discarded — a dead key like `^` has no other form: `adr/2026-08-normal-mode-compositions-reach-the-grammar.md`.)
- The sink's value is never read; printable keys are prevented so it stays empty, and it remounts clean with the block.
- Ctrl+C/X/V run through injected seams: the existing `Clipboard` read, and a new `ClipboardWrite` around `navigator.clipboard.writeText` (text sent over the eval channel, never interpolated into the script).
- Focus: the pane-focus effect (`adr/2026-08-the-pane-holds-focus.md`) now has two targets — the sink whenever a block is active and no overlay is up, the pane otherwise.

## Rejected

- **Composition on a non-editable div** — the IME never engages; the spike's earlier runs showed keys landing on `BODY` with no composition.
- **contenteditable** — rejected already in `adr/2026-07-hybrid-active-block-textarea.md`: the browser fights for the caret again, plus sanitisation.
- **A manual dead-key state machine on keydown** — reimplements the layout tables GTK owns and breaks the moment a layout differs.
