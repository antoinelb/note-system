#!/bin/sh
# The app indexes its own writes (adr/2026-09-the-app-indexes-its-own-writes.md):
# a note created seconds ago must already be findable by the picker and followable.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
today=$(date +%Y-%m-%d)

# Ctrl+N's overlay grabs focus through an async onmounted, and on a cold
# start the fresh vault's first typst compile can still be settling when
# the chord arrives — so each keystroke is paced behind a real screenshot
# round trip rather than fired blind (create-note.test.sh names the same
# trap).
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
e2e_type_paced "freshness proving ground"
e2e_key_paced Return

e2e_file_appears "permanent/freshness-proving-ground.typ"

# harness.sh carries no index-reading helper, so this scenario defines its
# own rather than editing it: the index is the assertion surface for
# findings #1/#8, a note on disk with no row in vault/.index/index.db
# under its own id is exactly the pre-fix bug this pins. The file itself
# is checked before opening it so polling never creates an empty db ahead
# of the app's own schema write.
e2e_index_query_holds() {
    test -f "$E2E_DIR/vault/.index/index.db" || return 1
    sqlite3 "$E2E_DIR/vault/.index/index.db" \
        "SELECT 1 FROM notes WHERE id = '$1';" 2>/dev/null | grep -q '^1$'
}
e2e_index_holds() {
    e2e_await e2e_index_query_holds "$1" \
        || e2e_fail "vault/.index/index.db never held a note with id '$1'"
}
# the assertion that fails against the pre-fix binary: the watcher's round
# trip has not landed yet, so the note is on disk but not in the index
e2e_index_holds "freshness-proving-ground"

# Ctrl+N's own Return auto-opens the new note's sheet
# (adr/2026-08-every-card-opens-the-sheet.md), whose textarea grabs focus
# through an async onmounted; a chord sent the instant that lands can race
# it and never reach a listener. go_logs/go_table, day-selection and a
# Return over a note that already exists are each side-effect-free from
# where they already stand, so resending the three is safe — e2e_await's
# own backoff is what supplies the settling a fixed sleep would only fake.
e2e_go_to_todays_daily() {
    e2e_key ctrl+2
    e2e_key ctrl+d
    # the empty day offers "no note for <day> — press enter to start one"
    e2e_key Return
    test -f "$E2E_DIR/vault/time/$today.typ"
}
e2e_await e2e_go_to_todays_daily \
    || e2e_fail "time/$today.typ was never written"

e2e_key i
# a non-writing motion absorbs the day note textarea's own just-focused
# race: End is a no-op on the fresh line the day note opens on, so the
# picker chord below lands on a textarea already proven to hold focus.
e2e_key End

# the picker asks for focus in its own async onmounted, "exactly as the
# textarea does" (adr/2026-08-ctrl-l-link-picker.md); a query typed before
# that grab lands would instead reach the still-focused day-note textarea
# (itself in insert mode, so at worst it types harmless literal text, never
# a vim command). Escape is safe on either side of that race — it closes an
# open picker "without writing" per the same ADR, or just drops the
# textarea back to Normal — so the fix is the same one already applied
# below: retry the whole open/type/accept sequence behind e2e_await instead
# of trusting one blind keystroke to have outrun the mount.
e2e_insert_link_via_picker() {
    e2e_key Escape
    e2e_key ctrl+l
    e2e_type "freshness"
    e2e_key Return
    grep -qaF "freshness-proving-ground" "$E2E_DIR/vault/time/$today.typ" \
        2>/dev/null
}
e2e_await e2e_insert_link_via_picker \
    || e2e_fail "time/$today.typ never held the link"

# the caret lands past the link the picker just spliced in — still inside
# the #l(...) call (links.rs: link_at treats just-past-')' as inside)
e2e_key ctrl+Return

# the sheet's textarea is a fresh mount grabbing focus through the same
# async onmounted as the day note's did; i sent before that lands never
# reaches the grammar, so the sentence below would be read as normal-mode
# commands instead of typed. Escape first is a safe reset whichever mode
# that left us in, and resending is harmless once insert really is
# active (i then becomes a literal, inert character ahead of the text
# the assertion actually looks for) — e2e_await's backoff is the wait.
e2e_types_the_sentence() {
    e2e_key Escape
    e2e_key i
    e2e_type "a distinctive sentence from create-then-follow"
    grep -qaF "a distinctive sentence from create-then-follow" \
        "$E2E_DIR/vault/permanent/freshness-proving-ground.typ" 2>/dev/null
}
e2e_await e2e_types_the_sentence || e2e_fail \
    "permanent/freshness-proving-ground.typ never held the sentence"

echo "ok: $(basename "$0")"
