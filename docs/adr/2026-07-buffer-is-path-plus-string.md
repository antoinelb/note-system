# The buffer is a path and a `String`, and the textarea owns the cursor

## Context

`plan.md` § Editor requires "a clean separation between the text buffer/edit-command layer and the widget layer, so a modal keymap can be inserted without rewriting the editor", and the roadmap starts that layer in phase 5.
The open question was how much of the edit-command layer to build now.

Phase 5's widget is a `<textarea>` — and it is scaffolding with a recorded deletion date (`adr/2026-07-plan-realigned-with-wireframes.md`): it dies in phase 7 when the logs centre pane lands.
The buffer under it does not die; it is the one part of this slice that survives into phase 7 and then into phase 8's hybrid block editor.

That inverts the usual instinct. The disposable half is the widget, so it should stay as dumb as possible; the durable half is the buffer, so its *invariant* matters more than its feature list.

## Decision

```rust
pub struct Buffer {
    file: PathBuf,   // absolute
    text: String,
}
```

Two fields, one invariant: **path and text travel together.**
`Buffer::open(file)` reads them as a pair and `save(&self)` writes them as a pair, so no caller can hand `save` a different path than `open` was given — the "typed in note A, switched fast, wrote it into note B" bug is unrepresentable rather than merely avoided.
The absolute path is stored rather than a vault-relative one plus a root, because a root parameter on `save` would reopen exactly that hole.

**No cursor and no edit operations in phase 5.** The textarea owns the cursor; `oninput` calls `set_text` with the whole value.
Cursor and `insert_char`/`delete_back`/`move_cursor` arrive in phase 8, which is the first code that can actually call them — the hybrid block editor needs cursor → block mapping, and by then the widget is ours rather than the browser's.

**No dirty flag.** The debounce timer only starts on an edit, so a tick implies an edit and there is no redundant write to suppress (`adr/2026-07-debounced-autosave.md`).

## Alternatives rejected

- **Cursor and edit ops now** — vim-shaped from day one, but the DOM textarea keeps its own cursor, so the two would need syncing through JS eval, and every op would need tests for a widget that cannot call it until phase 8. Under a 100 %-coverage rule, speculative code is not free: it is code plus tests for code nobody runs.
- **A rope (`ropey`)** — O(log n) edits, but at note scale a `String` memmove beats the bookkeeping, and it is a dependency the problem does not justify. The roadmap already called it YAGNI; this records the rejection.
- **A bare `Signal<String>` with no `Buffer` type** — genuinely tempting, since a newtype with no invariant is `String` with extra steps. Rejected because the invariant above is real: the path is what makes it a type rather than a wrapper.
- **Storing a vault-relative path plus a root field** — matches how `Note` and the index address files, but duplicates app-wide config into every buffer and gives `save` two ways to disagree with `open`.
