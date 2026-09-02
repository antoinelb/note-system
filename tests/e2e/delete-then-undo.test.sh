#!/bin/sh
# Ctrl+Shift+D deletes the open sheet's note with no confirmation and no
# trash (adr/2026-08-delete-note-chord.md), and the palette's "undo" row
# brings it back from the in-memory register, byte for byte, with its
# index row (adr/2026-08-app-level-undo-register.md). The file is the
# oracle both ways: gone from disk and from the index after the chord,
# back on both after the undo.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
NOTE="permanent/missing-type.typ"
before=$(cat "$E2E_DIR/vault/$NOTE")

# the keyboard-only way onto a sheet: the open-loops overlay's second row
# is missing-type, the first row that opens one
# (adr/2026-09-loop-lines-open-their-notes.md)
e2e_key_paced ctrl+p
e2e_type_paced "open loops"
e2e_key_paced Return
e2e_key_paced Down
e2e_key_paced Return

# guarded to an open sheet, so the chord over the table would do nothing:
# the file vanishing proves the sheet was open and the chord reached it
e2e_key_paced ctrl+shift+d
e2e_await test ! -f "$E2E_DIR/vault/$NOTE" \
    || e2e_fail "$NOTE survived ctrl+shift+d"
e2e_index_holds \
    "SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM notes WHERE id = 'missing-type');"

# the palette filters on the command's own label, "undo"; the row it
# renders wears the register's words, "undo delete missing-type". It is
# hidden while the register is empty, so a match at all proves the
# delete left its before-image behind
e2e_key_paced ctrl+p
e2e_type_paced "undo"
e2e_key_paced Return
e2e_file_appears "$NOTE"
[ "$(cat "$E2E_DIR/vault/$NOTE")" = "$before" ] \
    || e2e_fail "the undone note came back changed:
$(cat "$E2E_DIR/vault/$NOTE")"
# restored notes re-index under their real category in the same tick
# (adr/2026-09-the-app-indexes-its-own-writes.md)
e2e_index_holds \
    "SELECT 1 FROM notes WHERE id = 'missing-type' AND category = 'permanent';"

echo "ok: $(basename "$0")"
