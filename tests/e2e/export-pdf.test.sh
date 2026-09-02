#!/bin/sh
# The palette's "export pdf" writes the open note's pdf beside it
# (adr/2026-09-export-writes-the-pdf-beside-the-note.md), compiled the way
# the vanilla CLI compiles it. The file is the oracle: it appears next to
# the .typ and starts like a pdf.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# the logs open on today's empty day, which is no note to export; the
# open-loops overlay's second row opens missing-type's sheet
# (delete-then-undo.test.sh takes the same road)
e2e_key_paced ctrl+p
e2e_type_paced "open loops"
e2e_key_paced Return
e2e_key_paced Down
e2e_key_paced Return

e2e_key_paced ctrl+p
e2e_type_paced "export pdf"
e2e_key_paced Return
e2e_file_appears "permanent/missing-type.pdf"
head -c 5 "$E2E_DIR/vault/permanent/missing-type.pdf" | grep -q '^%PDF-' \
    || e2e_fail "permanent/missing-type.pdf does not start like a pdf"

echo "ok: $(basename "$0")"
