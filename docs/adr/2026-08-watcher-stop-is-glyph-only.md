# A stopped watcher changes only the liveness glyph

## Context

The watcher channel can close as the app launches, which made a persistent warning occupy the logs writing pane on every session.
The user does not need a textual watcher-stop notification, and the chrome already represents the same `Unwatched` state through its liveness glyph.

## Decision

When a running watcher channel closes, the app sets liveness to `Unwatched` and ends the receiver task without reporting a notice.
The sentence "the vault is no longer watched — outside edits will not appear" no longer exists in the application.
This decision supersedes the mid-session watcher-stop notice described by `2026-08-watcher-feeds-the-ui.md` and `2026-08-status-surface-owns-notices.md`; other watcher failures keep their existing notices.

## Alternatives rejected

- Keep the warning until Escape dismisses it: it repeatedly interrupts the primary writing surface with information the user does not need.
- Auto-dismiss the warning: it still adds a transient distraction and duplicates the liveness glyph.
- Remove the `Unwatched` state entirely: the state remains useful to the application and already has a compact representation in the chrome.
