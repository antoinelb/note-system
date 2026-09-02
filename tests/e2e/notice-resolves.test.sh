#!/bin/sh
# An index notice clears when its cause does
# (adr/2026-09-index-notices-resolve-on-a-good-lookup.md): following a
# dangling link raises the sticky "sheet: no note has the id ..." warning,
# Escape acknowledges it without wedging navigation, and once the missing
# note is created the same link resolves for real — no stale warning, no
# missing index row.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
today=$(date +%Y-%m-%d)

# Every keystroke followed by a real screenshot round trip to the X
# server — a genuine wall-clock pace tied to the compositor, never a
# guessed sleep — so a key sent right after an async onmounted focus grab
# does not outrun it (loop-line-opens-note.test.sh, table-chords.test.sh).
# The marker sentences below are still typed exactly once and never
# retried: a retry that fires after a slow-but-landed first send splices a
# duplicate into the buffer, so pacing the entry into insert mode is the
# fix, not re-sending the text.
e2e_key_paced() {
    e2e_key "$@"
    e2e_shot "$E2E_DIR/probe.png"
}

# Landing the caret on the #l("evergreen-notes") call takes k, f and a
# quotedbl motion, and every one of them crosses the same async
# onmounted focus grab every overlay in this suite hits — but a vim
# motion leaves no file behind to poll. The one thing that DOES change
# the instant the motion truly registers is the caret's own gold
# highlight, so e2e_await here polls a screenshot instead of guessing a
# duration: never a fixed sleep, and a k/f/quotedbl that lands on an
# unfocused sheet is a pure no-op, safe to resend until it isn't.
# (624, 393) is the opening quote of #l("evergreen-notes") on
# atomic-notes.typ's "Compare with ..." line, read once off a screenshot.
e2e_caret_gold_at() {
    convert "$1" -crop 1x1+"$2"+"$3" -format "%[fx:int(255*r)]" info: 2>/dev/null
}

# Two checkpoints, not one: k (a block-level, relative motion) and the
# f/quotedbl pair each get their own confirmed landing before the next
# motion fires, so a k that has already landed never gets resent under an
# f/quotedbl that raced it — every motion here is one gold check away
# from the next, never blind. (497, 390) is the "C" of "Compare with ...",
# k's own target; (624, 393) is the opening quote, f/quotedbl's.
e2e_caret_on_the_link() {
    e2e_shot "$E2E_DIR/probe.png"
    quote_gold=$(e2e_caret_gold_at "$E2E_DIR/probe.png" 624 393)
    if [ -n "$quote_gold" ] && [ "$quote_gold" -gt 150 ]; then
        return 0
    fi
    line_gold=$(e2e_caret_gold_at "$E2E_DIR/probe.png" 497 390)
    if [ -n "$line_gold" ] && [ "$line_gold" -gt 150 ]; then
        e2e_key f
        e2e_key quotedbl
    else
        e2e_key k
    fi
    return 1
}

# fixtures/vault/permanent/atomic-notes.typ links to "evergreen-notes",
# an id nothing in the vault owns — the open-loops overlay's dangling row
# names it by its source (loops.rs: the id on the line is the note that
# owes the link, not the missing target), so Enter on that row lands on
# atomic-notes' own sheet. This part is sent once, unretried, the same way
# create-then-follow.test.sh's own Ctrl+N chord needs no retry: it is the
# first overlay opened against an otherwise idle event loop, never the
# leg that actually races.
e2e_key ctrl+p
e2e_type "open loops"
e2e_key Return
e2e_key Down
e2e_key Down
e2e_key Return
e2e_await e2e_caret_on_the_link \
    || e2e_fail "the caret never reached the #l(\"evergreen-notes\") call"

# pre-fix, this raises the sticky "sheet: no note has the id
# evergreen-notes" warning and leaves it standing
e2e_key ctrl+Return
# the escape ladder's bottom rung acknowledges the notice without closing
# the sheet or the app going deaf to the keyboard
e2e_key Escape

# prove the app is still live and the notice did not wedge navigation:
# reaching today's daily note crosses the same focus-after-a-screen-switch
# race create-then-follow.test.sh's e2e_go_to_todays_daily hits, so the
# chord is resent — never a fixed sleep, e2e_await's own backoff supplies
# the settling.
e2e_reach_todays_daily() {
    e2e_key ctrl+d
    # the empty day offers "no note for <day> — press enter to start one"
    e2e_key Return
    test -f "$E2E_DIR/vault/time/$today.typ"
}
e2e_await e2e_reach_todays_daily \
    || e2e_fail "time/$today.typ was never written after the notice was dismissed"

# the day note's textarea grabs focus through the same async onmounted as
# every other overlay, but retrying "press i, type the sentence, press
# Escape" itself is not side-effect-free — table-chords.test.sh names this
# exact trap: a retry that fires after the first send actually landed
# (just slower than one check notices) resends i onto a cursor sitting
# right after its own still-settling insertion, splicing a second copy of
# the sentence into the first, and a substring grep still prints "ok" over
# that garbled duplicate. So the sentence is never retried; instead the i
# that enters insert mode is paced behind a screenshot round trip, so it
# lands on a textarea that has already had a compositor frame to take
# focus, and e2e_note_holds' own read-only grep (harness.sh) absorbs the
# autosave's debounce.
e2e_key_paced i
e2e_type "notice ladder proving ground"
e2e_key Escape
e2e_note_holds "time/$today.typ" "notice ladder proving ground"

# create the note the dangling link was missing; its title kebabs to
# exactly the id the link named. Mirrors create-then-follow.test.sh's own
# Ctrl+N flow, which needs no retry here for the same reason: this is the
# first overlay opened against an otherwise idle event loop.
e2e_key ctrl+n
e2e_type "concept"
e2e_key Return
e2e_type "evergreen notes"
e2e_key Return

e2e_file_appears "permanent/evergreen-notes.typ"

# harness.sh carries no index-reading helper, so this scenario defines its
# own, same as create-then-follow.test.sh: the file is checked before
# opening the db so polling never creates an empty one ahead of the app's
# own schema write.
e2e_index_query_holds() {
    test -f "$E2E_DIR/vault/.index/index.db" || return 1
    sqlite3 "$E2E_DIR/vault/.index/index.db" \
        "SELECT 1 FROM notes WHERE id = '$1';" 2>/dev/null | grep -q '^1$'
}
e2e_index_holds() {
    e2e_await e2e_index_query_holds "$1" \
        || e2e_fail "vault/.index/index.db never held a note with id '$1'"
}
e2e_index_holds "evergreen-notes"

# back to atomic-notes: the recent-notes picker still holds it from the
# first visit, filtered down to the one match. Sent once, unretried, same
# reasoning as the open-loops leg above — the overlay opening itself is
# not the leg that races; the vim motions into a just-mounted sheet are,
# so only e2e_caret_on_the_link below gets the screenshot-polled retry.
e2e_key Escape
e2e_key ctrl+b
e2e_type "atomic"
e2e_key Return
e2e_await e2e_caret_on_the_link || e2e_fail \
    "the caret never reached the #l(\"evergreen-notes\") call a second time"

# this is the assertion that fails against the pre-fix binary: there the
# stale warning and the missing index row both survive, so this link
# still opens an error sheet instead of the real note. Following it is
# itself a fresh sheet-open, its own async focus grab, so it gets the
# same screenshot-polled confirmation before typing: the "One note = one
# idea ..." paragraph is unique to atomic-notes, so its patch of the sheet
# goes dark the moment the sheet under the caret is really evergreen-notes.
e2e_paragraph_brightness() {
    e2e_shot "$E2E_DIR/probe2.png"
    convert "$E2E_DIR/probe2.png" -crop 206x16+494+310 \
        -format "%[fx:int(mean*255)]" info: 2>/dev/null
}

e2e_follow_the_resolved_link() {
    e2e_key ctrl+Return
    bright=$(e2e_paragraph_brightness)
    [ -n "$bright" ] && [ "$bright" -lt 40 ]
}
e2e_await e2e_follow_the_resolved_link \
    || e2e_fail "ctrl+Return never left atomic-notes' own sheet"

# same trap as the notice-ladder sentence above: the retry belongs only on
# the idempotent part (e2e_follow_the_resolved_link, confirming the
# resolved sheet is really the one on glass), never on "press i, type,
# press Escape" itself. The i is paced behind a screenshot so it lands on
# the resolved sheet's textarea after its focus grab; the sentence is
# typed exactly once, and e2e_note_holds' read-only grep absorbs the
# debounce — this is the leg that actually fails against the pre-fix
# binary, so it must confirm a clean, single-shot landing rather than a
# run that could pass by masking a bad one behind a resent, duplicated
# insert.
e2e_key_paced i
e2e_type "a distinctive sentence following the resolved link"
e2e_key Escape
e2e_note_holds "permanent/evergreen-notes.typ" \
    "a distinctive sentence following the resolved link"

echo "ok: $(basename "$0")"
