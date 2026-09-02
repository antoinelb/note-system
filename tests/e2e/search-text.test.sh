#!/bin/sh
# Full-text search (adr/2026-09-full-text-search-lives-in-the-index.md):
# Ctrl+Shift+F opens the finder over the vault's text, a word from a
# fixture note lists that note, and Enter opens it like a loop line would
# — a permanent note in a sheet. The sentence typed there is the oracle.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# "slips" is said by permanent/luhmann.typ alone ("paper slips"); the
# finder's input grabs focus in its own async onmounted, so the chord and
# the first keystroke after it are paced
e2e_key_paced ctrl+shift+f
e2e_type_paced "slips"
e2e_key_paced Return

# the hit opened luhmann's sheet; the caret rests on its last line
e2e_key_paced i
e2e_type "found through the finder"
e2e_key Escape
e2e_note_holds "permanent/luhmann.typ" "found through the finder"

echo "ok: $(basename "$0")"
