#!/bin/sh
# Ctrl+N's two-step overlay (adr/2026-08-ctrl-n-two-step-create-overlay.md):
# pick a type, give a title, and the note exists on disk under an id
# derived from that title.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

e2e_key ctrl+n
e2e_type "concept"
e2e_key Return
e2e_type "harness proving ground"
e2e_key Return

e2e_file_appears "permanent/harness-proving-ground.typ"
e2e_note_holds "permanent/harness-proving-ground.typ" 'type: "concept"'

echo "ok: $(basename "$0")"
