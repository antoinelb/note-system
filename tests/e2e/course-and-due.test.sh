#!/bin/sh
# The course type and the due loops
# (adr/2026-09-course-type-and-due-loops.md): Ctrl+N offers "course" as the
# ninth permanent type and writes the note from its template, and a note
# whose `due` day has gone by is an open loop whose line opens the note
# that owes it. The vault and the index are the oracles.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# the creator's two steps, each paced behind its focus grab
# (create-then-follow.test.sh names the same trap)
e2e_key_paced ctrl+n
e2e_type_paced "course"
e2e_key_paced Return
e2e_type_paced "analyse numerique"
e2e_key_paced Return
e2e_file_appears "permanent/analyse-numerique.typ"
e2e_note_holds "permanent/analyse-numerique.typ" 'type: "course"'
e2e_index_holds \
    "SELECT 1 FROM notes WHERE id = 'analyse-numerique' AND type = 'course';"

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
