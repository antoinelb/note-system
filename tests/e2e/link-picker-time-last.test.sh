#!/bin/sh
# The link picker offers permanent notes before time notes
# (adr/2026-09-time-notes-sort-last-in-the-link-picker.md). In the fixture
# vault the query "s" matches one time note and a dozen notes that are
# not, and the time note's id — "2026-summer" — sorts ahead of every one
# of them by id, so before this decision the highlighted first row was the
# season. Accepting the first row must now write "analyse-reelle", the
# first permanent note in the same order.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# today's day note, started the way wiki-link-trigger.test.sh starts it:
# each step is side-effect-free from where it already stands, so the
# retry behind e2e_await is safe
e2e_go_to_todays_daily() {
    e2e_key ctrl+2
    e2e_key ctrl+d
    e2e_key Return
    test -f "$E2E_DIR/vault/time/$E2E_TODAY.typ"
}
e2e_await e2e_go_to_todays_daily \
    || e2e_fail "time/$E2E_TODAY.typ was never written"
NOTE="$E2E_DIR/vault/time/$E2E_TODAY.typ"

e2e_key i
# End is a no-op on the fresh line the day note opens on, and absorbs the
# textarea's own just-focused race before the chord below
e2e_key End

# the picker grabs focus in an async onmounted, so the whole
# open/type/accept sequence is retried behind e2e_await exactly as
# create-then-follow.test.sh retries the same chord; the Return that
# lands the completion crosses that grab, so it is paced. Escape first is
# safe on either side of the race: it closes an open picker without
# writing, or drops the textarea back to normal mode.
e2e_accept_the_first_row() {
    e2e_key Escape
    e2e_key ctrl+l
    e2e_type "s"
    e2e_key_paced Return
    grep -qaF "[[analyse-reelle]]" "$NOTE" 2>/dev/null
}
e2e_await e2e_accept_the_first_row \
    || e2e_fail "time/$E2E_TODAY.typ never held the permanent note's link"

# the season is what the pre-decision order would have written
grep -qaF "[[2026-summer]]" "$NOTE" \
    && e2e_fail "the picker's first row was a time note"

# the index reads the link off the saved note
e2e_index_holds "SELECT 1 FROM links \
    WHERE source_path = 'time/$E2E_TODAY.typ' AND target_id = 'analyse-reelle';"

echo "ok: $(basename "$0")"
