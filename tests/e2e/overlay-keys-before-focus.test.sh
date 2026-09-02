#!/bin/sh
# A key typed at an overlay before its async focus grab lands is relayed
# into the overlay, never read by the grammar behind it
# (adr/2026-09-overlay-keys-relay-before-focus-lands.md). The chord and
# the type's letters go out back to back, faster than any focus grab: the
# pre-fix binary lost the "c" to the note behind, matched no type, and
# named the note "oncept...". Every other note is fingerprinted before and
# after, since a "c" or an "x" read as an operator would change one.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
today=$(date +%Y-%m-%d)

e2e_fingerprint() {
    (cd "$E2E_DIR/vault" && find . -name '*.typ' \
        ! -name 'typed-before-focus.typ' ! -name "$today.typ" \
        | sort | xargs cksum)
}
before=$(e2e_fingerprint)

xdotool key --clearmodifiers --delay 1 ctrl+n c o n c e p t
# Enter is the one key the relay drops rather than forwards (the ADR's
# known ceiling), so it waits for one screenshot round trip
e2e_shot "$E2E_DIR/probe.png"
e2e_key Return
e2e_type "typed before focus"
e2e_key Return

e2e_file_appears "permanent/typed-before-focus.typ"
e2e_note_holds "permanent/typed-before-focus.typ" 'type: "concept"'
[ "$(e2e_fingerprint)" = "$before" ] \
    || e2e_fail "a note behind the overlay changed"

echo "ok: $(basename "$0")"
