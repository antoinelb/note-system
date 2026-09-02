#!/bin/sh
# Alt+H folds the rail and Alt+L the jump panel
# (adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md). The screen is
# never the oracle here: what the run proves is that the two chords are
# taken by the app (typing after them lands in the note, not in a folded
# void) and that the palette rows run the same fold — the sentence typed
# after both folds and both unfolds is what decides.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# the empty day: the chords land on the pane itself, no sink holds them
e2e_key_paced alt+h
e2e_key_paced alt+l

# create the day; its note opens in normal mode, where the chords fold
# from the sink
e2e_reach_todays_daily() {
    e2e_key_paced ctrl+d
    e2e_key_paced Return
    test -f "$E2E_DIR/vault/time/$E2E_TODAY.typ"
}
e2e_await e2e_reach_todays_daily \
    || e2e_fail "time/$E2E_TODAY.typ was never written"
e2e_key_paced alt+h
e2e_key_paced alt+l

# the palette rows fold too; "fold rail" is typed whole so "fold jump
# panel" cannot be the highlighted row
e2e_key_paced ctrl+p
e2e_type_paced "fold rail"
e2e_key_paced Return

# and typing still lands in the note after every fold
e2e_key_paced i
e2e_type "typed with both panes folded and unfolded"
e2e_key Escape
e2e_note_holds "time/$E2E_TODAY.typ" "typed with both panes folded and unfolded"

echo "ok: $(basename "$0")"
