#!/bin/sh
# Ctrl+N's two-step overlay (adr/2026-08-ctrl-n-two-step-create-overlay.md):
# pick a type, give a title, and the note exists on disk under an id
# derived from that title.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# Ctrl+N's two-step overlay grabs focus through an async onmounted, and on
# a cold start the fresh vault's first typst compile can still be settling
# when the chord arrives — so each keystroke is paced behind a real
# screenshot round trip to the X server rather than fired blind, the same
# guard loop-line-opens-note.test.sh and notice-resolves.test.sh use.
e2e_key_paced() {
    e2e_key "$@"
    e2e_shot "$E2E_DIR/probe.png"
}
e2e_type_paced() {
    e2e_type "$1"
    e2e_shot "$E2E_DIR/probe.png"
}
e2e_key_paced ctrl+n
e2e_type_paced "concept"
e2e_key_paced Return
e2e_type_paced "harness proving ground"
e2e_key_paced Return

e2e_file_appears "permanent/harness-proving-ground.typ"
e2e_note_holds "permanent/harness-proving-ground.typ" 'type: "concept"'

echo "ok: $(basename "$0")"
