#!/bin/sh
# Ctrl+D opens today, Enter starts the note from its template, and what is
# typed into it reaches the file.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
today=$(date +%Y-%m-%d)

e2e_key ctrl+d
# the empty day offers "no note for <day> — press enter to start one"
e2e_key Return
e2e_file_appears "time/$today.typ"
e2e_note_holds "time/$today.typ" 'type: "daily"'

# without the i these are four motions and an open-line, which is what a
# first run of this harness actually recorded
e2e_key i
e2e_type "written under xdotool"
e2e_note_holds "time/$today.typ" "written under xdotool"

echo "ok: $(basename "$0")"
