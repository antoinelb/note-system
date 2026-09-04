#!/bin/sh
# Typing `[[` summons the link picker (adr/2026-09-wiki-links-replace-the-l-call.md):
# the empty pair the autopairs left is taken back out, so the accepted
# completion is the one `[[id]]` on the line and nothing doubles.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# today's day note, started the way create-then-follow.test.sh starts it:
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
e2e_key End

# the picker asks for focus in its own async onmounted; the whole
# open/type/accept sequence is retried behind e2e_await exactly as the
# Ctrl+L path is in create-then-follow.test.sh. Escape first is safe on
# either side of that race: it closes an open picker without writing, or
# drops the textarea back to normal mode.
e2e_link_via_typed_brackets() {
    e2e_key Escape
    e2e_key i
    e2e_key bracketleft
    e2e_key bracketleft
    e2e_type "luhm"
    e2e_key Return
    grep -qaF "[[luhmann]]" "$NOTE" 2>/dev/null
}
e2e_await e2e_link_via_typed_brackets \
    || e2e_fail "time/$E2E_TODAY.typ never held the typed link"

# exactly the link: no `[[]]` litter, no doubled brackets around it
grep -qaF "[[[[" "$NOTE" \
    && e2e_fail "the empty pair was left in front of the link"
grep -qaF "[[]]" "$NOTE" && e2e_fail "an empty pair survived"

# the index reads the link off the saved note
e2e_index_holds "SELECT 1 FROM links \
    WHERE source_path = 'time/$E2E_TODAY.typ' AND target_id = 'luhmann';"

echo "ok: $(basename "$0")"
