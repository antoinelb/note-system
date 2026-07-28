# Ctrl+Q quits by flushing the buffer, then closing the window

## Context

Saves are debounced (`QUIET`), so a quit pressed mid-sentence lands inside
the quiet window and would drop the last keystrokes. Removing the native
menu made Ctrl+Q the only in-app quit path, so it has to own this problem.

## Decision

- Ctrl+Q synchronously flushes the open buffer, then closes the window.
- A flush that fails **cancels the quit** and shows the save error — the
  app never closes over an unsaved buffer, and never loses it silently.
- The chord lives on the `.app` root next to Ctrl+T (focus sits there);
  `Shell` registers its flush through a context cell (`QuitFlush`), since
  the buffer is `Shell` state the root handler cannot reach directly.
- The window close is injected from `main` as `Closer` — the headless test
  `VirtualDom` has no `DesktopContext`, so tests inject a recorder through
  the same root-context channel as `VaultRoot`.

## Rejected

- Close immediately, rely on the debounced autosave: loses up to `QUIET`
  of typing on every quit.
- Quit even when the flush fails: silent data loss; the no-hard-blocks
  principle governs saving, not destroying.
- Lifting the buffer into `App` so the handler reaches it directly:
  restructures working editor state for one keystroke.

## Note

The window-manager close button (and any WM kill) still bypasses the
flush — worst case loses `QUIET` of typing. Wiring the window close event
through the same flush is future work if it ever bites.
