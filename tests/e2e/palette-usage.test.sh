#!/bin/sh
# The palette orders by how often a command was run, and the counts are
# user data in a plain-lines file beside the positions
# (adr/2026-09-palette-orders-by-usage.md). The vault is the oracle: run
# "toggle theme" twice from the palette and `.index/usage` must hold
# `toggle-theme 2` — the count, its file, and the atomic write that puts
# it there, all proved on disk rather than on pixels.
#
# "toggle theme" is the harmless one: it moves a session-only signal
# (adr/2026-08-settings-overlay.md) and writes nothing else.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
DAY="time/$E2E_TODAY.typ"

# a note on screen first, so the palette opens over the app's ordinary
# state rather than over a bare boot
e2e_key_paced ctrl+d
# the empty day offers "no note for <day> — press enter to start one"
e2e_key_paced Return
e2e_file_appears "$DAY"

# Ctrl+P and the query both cross the overlay's async focus grab, so both
# are paced behind a screenshot round trip (harness.sh, e2e_key_paced)
e2e_key_paced ctrl+p
e2e_type_paced "theme"
e2e_key_paced Return

# nothing counted before this run, so the first save creates the file
e2e_file_appears ".index/usage"

# and again: the same row, now at the head of the list
e2e_key_paced ctrl+p
e2e_type_paced "theme"
e2e_key_paced Return

e2e_note_holds ".index/usage" "toggle-theme 2"

# no litter beside it: the write goes temp-then-rename
test ! -e "$E2E_DIR/vault/.index/usage.tmp" \
    || e2e_fail "the atomic write left usage.tmp behind"

echo "ok: $(basename "$0")"
