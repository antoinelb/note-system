#!/bin/sh
# A key typed before the sink's focus grab lands reaches a pane, and the
# pane reads it as the sink would instead of dropping it
# (adr/2026-09-the-sink-outlives-the-active-block.md). Two grabs are
# crossed unpaced on purpose: the note's first mount (Ctrl+D creating the
# day, then o and the sentence in one burst) and an overlay's close
# (Ctrl+O landing on a note, then i and the sentence in one burst).
# Pacing here would hide the defect this exists to catch: the settings
# scenario paced its o and still held "ettings proving ground" one run in
# four under load, the o having fallen on the pane.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
DAY="time/$E2E_TODAY.typ"

e2e_key ctrl+d
e2e_key Return
e2e_file_appears "$DAY"
e2e_key o
e2e_type "the first letter after the day mounts"
e2e_key Escape
e2e_note_holds "$DAY" "the first letter after the day mounts"

# the switcher: its query is typed unpaced too — the relay owns a key
# that beats its grab — and the landing's i and letters go in one burst
e2e_key ctrl+o
e2e_type "luhmann"
e2e_key_paced Return
e2e_key i
e2e_type "the first letter after the switcher closes"
e2e_key Escape
e2e_note_holds "permanent/luhmann.typ" "the first letter after the switcher closes"

echo "ok: $(basename "$0")"
