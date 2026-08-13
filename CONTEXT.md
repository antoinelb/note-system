# note-system

A personal Typst knowledge system: plain `.typ` files as the source of truth, a derived index, two screens.
This glossary is the canonical language; it defines what things are, never how they work.

## Surfaces

**Table**:
The canvas screen of permanent notes as positioned cards.
_Avoid_: canvas, board

**Logs**:
The time screen — rail, centre note, month grid.
_Avoid_: journal view, calendar screen

**Sheet**:
The modal card-editing surface a table card opens into.
_Avoid_: modal, dialog

**Chrome line**:
The app's single top line — two screen icons, the ember, and the liveness glyph.
_Avoid_: toolbar, header, status bar

**Ember**:
The count of open loops shown on the chrome line.
_Avoid_: badge, debt counter

## Status

**Status**:
The module that owns every user-facing message and the liveness fact; nothing else displays a failure.
_Avoid_: error handler, notification system

**Notice**:
One user-facing message with a severity, shown on the in-pane notice line and kept in the notice history.
_Avoid_: toast, alert, error message

**Severity**:
A notice's rank — info, warning, or critical — deciding what may replace it and how it leaves the screen.
_Avoid_: priority, level

**Acknowledgement**:
The explicit gesture (Escape at the bottom of the escape ladder) that lets a critical notice leave the screen.
_Avoid_: dismissal (that is for warnings), click-through

**Resolution**:
The condition a notice reported ceasing to hold (a later clean save, a healthy batch); resolution clears the notice without a gesture.
_Avoid_: auto-dismiss, timeout

**Liveness**:
The fact of whether the index tracks the vault: Watching, Unwatched, or Degraded.
_Avoid_: connection status, sync state, health

**Liveness glyph**:
The mark on the chrome line rendering liveness in the same place in every state — dim when Watching, bright when not.
_Avoid_: indicator, icon (unqualified)

## Persistence

**Atomic write**:
A save that lands whole or not at all; a crash mid-write leaves the previous version, never a truncation.
_Avoid_: safe write, journaled write

**Stamp**:
The mtime of the version the open buffer last read or wrote — the guard's reference point for divergence.
_Avoid_: timestamp (unqualified), version

**Conflict**:
The open note having changed on disk under the buffer: the save refuses, both versions survive, and only the user can pick a side.
_Avoid_: merge conflict (nothing merges), collision

**Keep mine / Take disk**:
The conflict's two resolutions, both palette commands: the buffer overwrites the disk, or the disk reloads the buffer.
_Avoid_: force save / revert
