#!/bin/sh
# The keyboard sink mounts once with the shell and never remounts when
# another block wakes (adr/2026-09-the-sink-is-the-one-keyboard-socket.md).
# Before, `o` unmounted it with the block it sat in and the first letter
# typed before the fresh sink's focus grab landed fell on <body>:
# `settings-overlay.test.sh` held "ettings proving ground" one run in
# four under load. So this scenario is unpaced on purpose — the wake and
# the letters go in one burst, twice: `o` from the day's trailing line,
# then `k` and `A` from the line it opened. Pacing here would hide the
# defect it exists to catch.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
DAY="time/$E2E_TODAY.typ"

e2e_key ctrl+d
e2e_key Return
e2e_file_appears "$DAY"

e2e_key o
e2e_type "sink survives the wake"
e2e_key Escape
e2e_note_holds "$DAY" "sink survives the wake"

# k wakes the line above, A appends at its end: the first letter after a
# wake by motion lands too
e2e_key k
e2e_key A
e2e_type " and the line above kept its first letter"
e2e_key Escape
e2e_note_holds "$DAY" " and the line above kept its first letter"

echo "ok: $(basename "$0")"
