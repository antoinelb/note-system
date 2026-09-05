#!/bin/sh
# The settings overlay (Ctrl+,, adr/2026-08-settings-overlay.md) opens
# over the note, owns every bare key while it stands, and hands the
# keyboard back to the note when Escape closes it. Its controls are
# session-only by decision and leave nothing on disk, so what the vault
# can witness is the overlay's presence: dd typed while it stands is
# dropped by the overlay's own reader (adr/2026-09-the-sink-is-the-one-keyboard-socket.md)
# and the line survives — an overlay that never opened would have let dd
# delete it — and the sentence appended after Escape proves the note took
# the keyboard back (adr/2026-08-the-pane-holds-focus.md).
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
DAY="time/$E2E_TODAY.typ"

e2e_key ctrl+d
# the empty day offers "no note for <day> — press enter to start one"
e2e_key Return
e2e_file_appears "$DAY"

e2e_key_paced o
e2e_type "settings proving ground"
e2e_key Escape
e2e_note_holds "$DAY" "settings proving ground"

# the chord, then dd in the same burst: with the overlay up the keys are
# dropped, and the line below still holds
e2e_key ctrl+comma
e2e_key d
e2e_key d
e2e_shot "$E2E_DIR/probe.png"
e2e_key_paced Escape

# A appends at the line's end, so a dropped chord (dd deleting the line)
# and a keyboard stranded on <body> (nothing appended) both fail here
e2e_key_paced A
e2e_type " and typing resumed after settings closed"
e2e_key Escape
e2e_note_holds "$DAY" \
    "settings proving ground and typing resumed after settings closed"

echo "ok: $(basename "$0")"
