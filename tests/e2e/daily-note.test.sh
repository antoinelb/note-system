#!/bin/sh
# Ctrl+D opens today, Enter starts the note from its template, and what is
# typed into it reaches the file.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

e2e_key_paced ctrl+d
# the empty day offers "no note for <day> — press enter to start one"
e2e_key_paced Return
e2e_file_appears "time/$E2E_TODAY.typ"
e2e_note_holds "time/$E2E_TODAY.typ" 'type: "daily"'

# without the i these are four motions and an open-line, which is what a
# first run of this harness actually recorded
e2e_key_paced i
e2e_type "written under xdotool"
e2e_note_holds "time/$E2E_TODAY.typ" "written under xdotool"

echo "ok: $(basename "$0")"
