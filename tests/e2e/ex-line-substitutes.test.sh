#!/bin/sh
# The ex line is literal and always global
# (adr/2026-08-ex-line-is-literal-and-global.md): `:s/old/new/` replaces
# every occurrence on the caret's line with no regex and no g flag. The
# colon reaches the grammar through the sink and opens the prompt; the
# command's letters go out in the same xdotool burst, ahead of the
# prompt's focus grab, and are relayed into its query
# (adr/2026-09-overlay-keys-relay-before-focus-lands.md). Only the
# Return, the one key the relay drops, is paced.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
DAY="time/$E2E_TODAY.typ"

e2e_key ctrl+d
# the empty day offers "no note for <day> — press enter to start one"
e2e_key Return
e2e_file_appears "$DAY"

e2e_key_paced o
e2e_type "alpha then beta then alpha again"
e2e_key Escape
e2e_note_holds "$DAY" "alpha then beta then alpha again"

# both occurrences on the line, with no g flag to ask for the second
e2e_type ":s/alpha/gamma/"
e2e_shot "$E2E_DIR/probe.png"
e2e_key Return
e2e_note_holds "$DAY" "gamma then beta then gamma again"
grep -q "alpha" "$E2E_DIR/vault/$DAY" \
    && e2e_fail "an alpha survived :s/alpha/gamma/"

# literal, not a regex: the dot matches only a dot, so the line stands
e2e_type ":s/./X/"
e2e_shot "$E2E_DIR/probe.png"
e2e_key Return
# a refused or misread command speaks through the status surface, which
# the vault cannot see; what it can see is the line still standing after
# a change that would have rewritten it — proven by a later edit landing
e2e_key_paced A
e2e_type " and a dot."
e2e_key Escape
e2e_note_holds "$DAY" "gamma then beta then gamma again and a dot."

echo "ok: $(basename "$0")"
