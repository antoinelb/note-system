# Ctrl+T toggles the todo checkbox, not the theme

## Context

`2026-07-theme-keystroke-toggle.md` gave Ctrl+T to the theme.
Daily use turned out to want a checkbox toggle on the line the caret sits on far more often than a theme switch, and Ctrl+T is the natural chord for "todo" — the theme has no comparable mnemonic claim on it.
Freeing the chord means the theme still needs a way to change: the palette already lists "toggle theme" as a row, so nothing new has to be invented there.

## Decision

Ctrl+T is the todo toggle, wired at pane level in both editors — the logs screen's `keyboard` closure and the table sheet's `table_keys` closure — the same way Ctrl+L opens the link picker: a guard on `editor.peek().active().is_some()`, not a `keymap::Action`, because it edits the line the caret sits on and belongs to the buffer, never the vim grammar (CLAUDE.md's buffer/widget separation; the grammar already passes every ctrl chord through as `Outcome::Pass`, `src/vim.rs`).

`Editor::toggle_todo` reads the caret's physical line and rewrites it with a pure helper, `todo_toggled`:

- `- [ ]` and `- [x]` swap directly, tail untouched — the checkbox is never removed.
- A `-`/`+` item (bare, or followed by text) is promoted straight to `- [ ] `, so ordered `+` items are not mangled into something else.
- Anything else — prose, an empty line — is prefixed into a fresh unchecked item.

Every branch only grows or keeps the line's byte length, which lets the caret's post-splice position be computed as `head + (replacement.len() - line.len())` without a checked subtraction.

`toggle_todo` checkpoints itself before splicing. Every other buffer-side edit in the typing path (autopairs, list continuation) rides an insert session's own entry checkpoint and never calls `checkpoint()` directly; the todo chord fires from normal mode, with no insert session open to have already paid for it, so without its own call `u` after Ctrl+T would undo the whole prior insert session along with the toggle — two change intents as one step, against `adr/2026-08-undo-at-vim-grain.md`.

The theme keeps only its palette row (`palette::CommandId::ToggleTheme`, `chord: None`); the App-root `onkeydown` handler that used to intercept Ctrl+T is deleted, and the chord bubbles down to the panes that now claim it.

The chord is deliberately **not** a new palette command — the plan's out-of-scope rule caps new palette commands at the nine navigation ones this same round of fixes adds — so `palette`'s completeness audit (`the_registered_set_matches_the_apps_chords`) carries one documented exception: Ctrl+T answers to `toggle_todo`, never to a `CommandId`.

## Rejected

- **Keep Ctrl+T on the theme, give the todo toggle a different chord** — Ctrl+T is the mnemonic users reach for; fighting that with the older, less-used binding recreates the same collision from the other side.
- **A `keymap::Action` variant for the todo toggle** — the grammar's job is modal *navigation and structure* (`d`, `w`, visual selection); text mutation driven by what character sits under the caret is the editor's job, the same reasoning that keeps autopairs and list-continuation out of `vim.rs` (`adr/2026-08-autopairs-in-the-typing-path.md`).
- **A palette command for the toggle** — out of scope for this round by the plan's own rule, and a chord this frequent (every checked-off task, every day) earns muscle memory over a menu row the other eight navigation commands do not compete for.
