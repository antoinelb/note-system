#!/bin/sh
# Ctrl+O is the one note switcher
# (adr/2026-09-ctrl-o-is-the-one-note-switcher.md): from any screen it
# opens one overlay whose rows are the visit log while the query is empty
# and every note in the index once a character is typed, and whose landing
# follows the loops list's category rule — a time note to the logs,
# everything else to a sheet.
#
# The vault is the oracle (adr/2026-08-headless-x11-e2e.md): each leg
# writes a distinctive sentence into the note the switcher was supposed to
# land on, so a switch that went anywhere else leaves that note untouched
# and the run fails on the file, never on a pixel.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# Every key here crosses an async focus grab — the overlay's own
# onmounted, then the sheet's or the logs pane's after the landing — so
# each is paced behind one screenshot round trip, the rule
# harness.sh states. The sentence itself is never resent.
e2e_switch_to() {
    e2e_key_paced ctrl+o
    e2e_type_paced "$1"
    e2e_key_paced Return
}

e2e_write_into_the_note() {
    e2e_key_paced i
    e2e_type "$1"
    e2e_key Escape
}

# 1. a typed query reaches a note never visited and with no rail row of
#    its own: the recent-notes picker this replaces could not name it at
#    all, having no visit to remember.
e2e_switch_to "luhmann"
e2e_write_into_the_note "switched here by name"
e2e_note_holds "permanent/luhmann.typ" "switched here by name"

# 2. and again from the sheet the first switch opened, so the log has two
#    entries to read back
e2e_switch_to "plain-files"
e2e_write_into_the_note "and on to the second note"
e2e_note_holds "permanent/plain-files.typ" "and on to the second note"

# 3. the empty query is the visit log, newest first, the sheet currently
#    showing left out: the first row is luhmann, the note step 2 left.
#    Enter with nothing typed is therefore the back gesture Ctrl+B was.
e2e_key_paced ctrl+o
e2e_key_paced Return
e2e_write_into_the_note "back through the recent list"
e2e_note_holds "permanent/luhmann.typ" "back through the recent list"

# 4. from the table's sheet, a time note lands on the logs with its day
#    selected — the category rule, and the destination the table-only
#    jump-to-card had no way to reach
e2e_switch_to "2026-07-22"
e2e_write_into_the_note "landed on the day from the table"
e2e_note_holds "time/2026-07-22.typ" "landed on the day from the table"

echo "ok: $(basename "$0")"
