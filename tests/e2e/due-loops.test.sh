#!/bin/sh
# The due loops (adr/2026-09-course-type-and-due-loops.md): a note whose
# `due` day has gone by is an open loop whose line opens the note that
# owes it. The vault and the index are the oracles.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# the fixture's devoir-1 is due 2026-07-20, four days before E2E_TODAY: an
# overdue loop. The index carries the day the loops list judges against
e2e_index_holds \
    "SELECT 1 FROM notes WHERE id = 'devoir-1' AND due = '2026-07-20';"

# the due families sit last in the loops list and the highlight clamps at
# its end (adr/2026-09-loop-lines-open-their-notes.md), so a surplus of
# Down presses lands on the overdue line whatever the rest of the debt
# counts to. Only the first Down follows the overlay's focus grab
e2e_key_paced ctrl+2
e2e_key_paced ctrl+p
e2e_type_paced "open loops"
e2e_key_paced Return
e2e_key_paced Down
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do
    e2e_key Down
done
e2e_key_paced Return

# the line opened devoir-1's sheet: the sentence typed into it decides
e2e_key_paced i
e2e_type "handed in, from the overdue loop line"
e2e_key Escape
e2e_note_holds "permanent/devoir-1.typ" "handed in, from the overdue loop line"

echo "ok: $(basename "$0")"
