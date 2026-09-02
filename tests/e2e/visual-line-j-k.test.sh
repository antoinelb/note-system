#!/bin/sh
# j and k walk the lines the webview draws, not the physical lines
# (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md): the one
# behaviour in the app that lives in `launch::LINE_WALK`, a script only a
# real webview runs. A single physical line long enough to wrap at the
# logs column (min(529px, 100%)) is typed into the day note; j from its
# start must land inside that same line, one drawn row down, and k from
# there back up onto the first row — a physical-line j at the note's last
# line is a no-op and would leave both markers at the line's start.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
DAY="time/$E2E_TODAY.typ"

e2e_key_paced ctrl+d
# the empty day offers "no note for <day> — press enter to start one"
e2e_key_paced Return
e2e_file_appears "$DAY"

# o opens a fresh physical line below the template's last one, so the
# long line is its own block; forty numbered words wrap into several
# drawn rows at any prose size the settings overlay offers
long=$(seq -f 'w%02g' 1 40 | tr '\n' ' ')
e2e_key_paced o
e2e_type "$long"
e2e_key Escape
e2e_note_holds "$DAY" "w01 w02 w03"

# 0 then j: one drawn row down from the line's first column, still inside
# the physical line; i inserts the marker where the walk landed
e2e_key 0
e2e_key_paced j
e2e_key_paced i
e2e_type "DOWN "
e2e_key Escape
e2e_note_holds "$DAY" "DOWN "

# k from the marker: one drawn row up, onto the first row, at the goal
# column the walk held — after w01, never at the line's start
e2e_key_paced k
e2e_key_paced i
e2e_type "UP "
e2e_key Escape
e2e_note_holds "$DAY" "UP "

# both markers sit inside the one physical line, in row order, with the
# line's first word still first: j and k each moved one drawn row
grep -q '^w01.*UP .*DOWN .*w40' "$E2E_DIR/vault/$DAY" || e2e_fail \
    "the markers did not land one drawn row apart inside the long line; it holds:
$(grep -n 'w01' "$E2E_DIR/vault/$DAY")"
[ "$(grep -c 'DOWN ' "$E2E_DIR/vault/$DAY")" = 1 ] \
    || e2e_fail "the DOWN marker was typed more than once"

echo "ok: $(basename "$0")"
