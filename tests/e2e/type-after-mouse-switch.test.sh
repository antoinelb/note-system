#!/bin/sh
# Finding #4: a mouse-driven screen switch must not swallow the keystrokes
# typed right after it. The pane's focus effect now reads `sheet` and
# `screen` on every run, so a chrome-icon click's own onmounted request is
# backed by the effect re-firing on the switch itself, not just racing
# WebKitGTK's native click-focus
# (adr/2026-09-sheet-and-screen-join-the-focus-effect.md). The three
# gestures — icon, palette, chord — all run the same `go_table`/`go_logs`
# callbacks, so proving the icon path proves the seam
# (adr/2026-08-screen-switch-gesture.md). Pre-fix, the click left focus on
# <body>: `i` opened nothing and the sentence below never reached the file.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
today=$(date +%Y-%m-%d)

# the typing target has to exist before either mouse leg runs
e2e_key ctrl+d
# the fixture ships no note for today
e2e_key Return
e2e_file_appears "time/$today.typ"

# leg 1: table icon, then the logs icon back, with no keyboard gesture
# between the two clicks — `i` and prose come immediately after the second
e2e_click_chrome table
e2e_click_chrome logs
e2e_key i
e2e_type "typed right after the click back to the logs icon"
e2e_note_holds "time/$today.typ" \
    "typed right after the click back to the logs icon"

# leg 2: the table icon, then Ctrl+D with nothing in between — the chord
# only reaches the table pane's own keydown handler if the click actually
# left focus there, so `i` and prose right after prove it did
e2e_click_chrome table
e2e_key ctrl+d
e2e_key i
e2e_type "typed after a table click then ctrl+d"
e2e_note_holds "time/$today.typ" "typed after a table click then ctrl+d"

echo "ok: $(basename "$0")"
