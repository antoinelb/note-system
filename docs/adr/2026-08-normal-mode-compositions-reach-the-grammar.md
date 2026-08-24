# A normal-mode composition commits one keystroke to the grammar

## Context

`^` — vim's first-non-blank motion — did not work at all, standalone or behind a verb.

On a French layout `^` is a dead key: it never arrives as a keydown, only as a composition, and it only commits once a second key follows (a space, or a second `^`).
The sink discarded that: `oncompositionend` reached the buffer only in insert mode, because "a composition a normal-mode key started is discarded whole" (`adr/2026-08-hidden-ime-sink.md`).

That rule was written when normal mode had nothing to do with text.
It is still right about what a composition must not do — write prose into the note — but wrong about what it *is*: for a dead key, the commit is the only form the keystroke ever takes.

## Decision

Outside insert mode, a **single-cluster** composition commit is handed to the grammar as `Key::Character`, exactly as a keydown would be; anything longer is still discarded whole.

- One cluster is one keystroke. A real IME committing a word or a phrase is not a chord, and the old rule keeps it out.
- The commit goes through the same `grammar` → `apply_vim` path as a keydown, so it checkpoints and is one undo step like every other change intent (`adr/2026-08-undo-at-vim-grain.md`) — a composed `^` behind `d` deletes text, and `u` puts it back.
- Only `Outcome::Acts` acts. `Pass` and `Swallow` drop the commit, so normal mode still never inserts what it does not bind.
- The three callers that need a `View` snapshot — the sink's keydown, this commit, and search's synthesized `n` — share one `grammar` callback rather than a third copy of the same fifteen lines.
- The cost is accepted: `^` takes two keystrokes in normal mode, because the browser cannot say *which* dead key was pressed until it commits.

## Rejected

- **Recognising the dead key from `event.code()`** — one keystroke instead of two, but it bakes one keyboard's physical layout into the source. `adr/2026-08-hidden-ime-sink.md` already rejected the same idea in its "manual dead-key state machine" form, for the same reason: it reimplements the layout tables GTK owns and breaks the moment a layout differs.
- **Feeding the `compositionupdate` character and cancelling the composition** — one keystroke, but cancelling a composition mid-flight is not reliably possible on WebKitGTK, and the next letter would compose against a half-torn-down state.
- **Binding first-non-blank to a key that is not dead** — cheapest of all, and it stops being vim.
- **Passing every commit through, not just single-cluster ones** — a real IME's word commit would be read as a chord, and the first key of it would fire something.
