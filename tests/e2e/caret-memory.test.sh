#!/bin/sh
# A note reopens with the caret where it was left
# (adr/2026-09-a-note-reopens-where-it-was-left.md). The vault is the
# oracle: `.index/carets` must hold `<path> <line> <column>` for the note
# just left, and the entry must be replaced — not appended — the second
# time the same note is left from a different line.
#
# The restore is proven by the second entry alone: the note is re-entered
# and walked two lines further with no other motion, so `4 0` can only
# come from having landed back on line 2.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
DAY="time/$E2E_TODAY.typ"

# today's note from its template, on the logs
e2e_key_paced ctrl+d
# the empty day offers "no note for <day> — press enter to start one"
e2e_key_paced Return
e2e_file_appears "$DAY"

# gg then two j: line 2 of the preamble, column 0. Every line walked here
# is short, so a visual line and a logical one are the same line.
e2e_walk_two_lines() {
    e2e_key_paced j
    e2e_key_paced j
}
e2e_key_paced g
e2e_key_paced g
e2e_walk_two_lines

# leaving the day for a permanent note is what writes the entry
e2e_leave_for_luhmann() {
    e2e_key_paced ctrl+o
    e2e_type_paced "luhmann"
    e2e_key_paced Return
}
e2e_leave_for_luhmann
e2e_file_appears ".index/carets"
e2e_note_holds ".index/carets" "$DAY 2 0"

# back to the day: the caret lands on line 2 again, so two more j put it
# on line 4 — and leaving writes that place over the old one
e2e_key_paced ctrl+o
e2e_type_paced "$E2E_TODAY"
e2e_key_paced Return
e2e_walk_two_lines
e2e_leave_for_luhmann
e2e_note_holds ".index/carets" "$DAY 4 0"

grep -qF "$DAY 2 0" "$E2E_DIR/vault/.index/carets" \
    && e2e_fail "the entry was appended, not replaced:
$(cat "$E2E_DIR/vault/.index/carets")"

# no litter beside it: the write goes temp-then-rename
test ! -e "$E2E_DIR/vault/.index/carets.tmp" \
    || e2e_fail "the atomic write left carets.tmp behind"

echo "ok: $(basename "$0")"
