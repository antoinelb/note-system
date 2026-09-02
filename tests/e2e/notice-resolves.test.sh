#!/bin/sh
# An index notice clears when its cause does
# (adr/2026-09-index-notices-resolve-on-a-good-lookup.md): following a
# dangling link raises the sticky "sheet: no note has the id ..." warning,
# Escape acknowledges it without wedging navigation, and once the missing
# note is created the same link resolves for real — no stale warning, no
# missing index row.
#
# The vault is the only oracle here (adr/2026-08-headless-x11-e2e.md):
# an earlier version of this scenario read the caret's gold highlight and
# a paragraph's brightness off screenshots to confirm each motion had
# landed before the next was sent. Every focus grab those probes guarded
# against is now paced behind one screenshot round trip instead
# (harness.sh `e2e_key_paced`), and the sentence typed at the end decides
# the run: it lands in evergreen-notes.typ only if the link resolved.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# fixtures/vault/permanent/atomic-notes.typ links to "evergreen-notes",
# an id nothing in the vault owns — the open-loops overlay's dangling row
# names it by its source (loops.rs: the id on the line is the note that
# owes the link, not the missing target), so Enter on that row (rank 2,
# after missing-meta and missing-type) lands on atomic-notes' own sheet.
# The sheet opens with the caret on its empty last line
# (adr/2026-08-cursor-always-in-the-note.md); k climbs to "Compare with
# #l("evergreen-notes")." and f-quotedbl lands on the call's opening
# quote. Each motion crosses the sheet's own focus grab, so each is paced.
e2e_caret_onto_the_link() {
    e2e_key_paced k
    e2e_key_paced f
    e2e_key_paced quotedbl
}

e2e_key_paced ctrl+p
e2e_type_paced "open loops"
e2e_key_paced Return
e2e_key_paced Down
e2e_key_paced Down
e2e_key_paced Return
e2e_caret_onto_the_link

# pre-fix, this raises the sticky "sheet: no note has the id
# evergreen-notes" warning and leaves it standing
e2e_key_paced ctrl+Return
# the escape ladder's bottom rung acknowledges the notice without closing
# the sheet or the app going deaf to the keyboard
e2e_key_paced Escape

# prove the app is still live and the notice did not wedge navigation:
# Ctrl+D then Enter over the empty day is idempotent on disk, so the pair
# is resent until the file exists. Both are paced so the first attempt
# has landed before the check, and a resend stays the exception: a
# resent Enter over the note it just opened reactivates its block, and
# the block's remount swallows the first letters typed into it
e2e_reach_todays_daily() {
    e2e_key_paced ctrl+d
    # the empty day offers "no note for <day> — press enter to start one"
    e2e_key_paced Return
    test -f "$E2E_DIR/vault/time/$E2E_TODAY.typ"
}
e2e_await e2e_reach_todays_daily \
    || e2e_fail "time/$E2E_TODAY.typ was never written after the notice was dismissed"
e2e_note_holds "time/$E2E_TODAY.typ" 'type: "daily"'

# the i that enters insert mode is paced behind the day note's focus
# grab; the sentence is typed exactly once
e2e_key_paced i
e2e_type "notice ladder proving ground"
e2e_key Escape
e2e_note_holds "time/$E2E_TODAY.typ" "notice ladder proving ground"

# create the note the dangling link was missing; its title kebabs to
# exactly the id the link named
e2e_key_paced ctrl+n
e2e_type_paced "concept"
e2e_key_paced Return
e2e_type_paced "evergreen notes"
e2e_key_paced Return
e2e_file_appears "permanent/evergreen-notes.typ"
e2e_index_holds "SELECT 1 FROM notes WHERE id = 'evergreen-notes';"

# back to atomic-notes: the recent-notes picker still holds it from the
# first visit, filtered down to the one match
e2e_key_paced Escape
e2e_key_paced ctrl+b
e2e_type_paced "atomic"
e2e_key_paced Return
e2e_caret_onto_the_link

# this is the leg that fails against the pre-fix binary: there the stale
# warning and the missing index row both survive, so this link still
# opens an error sheet instead of the real note, and the sentence below
# lands anywhere but evergreen-notes.typ
e2e_key_paced ctrl+Return
e2e_key_paced i
e2e_type "a distinctive sentence following the resolved link"
e2e_key Escape
e2e_note_holds "permanent/evergreen-notes.typ" \
    "a distinctive sentence following the resolved link"

echo "ok: $(basename "$0")"
