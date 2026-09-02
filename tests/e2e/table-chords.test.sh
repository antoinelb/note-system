#!/bin/sh
# Table-screen chords after a sheet closes (findings #3/#6, fixed by the
# focus effect reading `sheet` and `screen`,
# adr/2026-09-sheet-and-screen-join-the-focus-effect.md): Ctrl+D, Ctrl+2
# and Ctrl+P all still reach the table pane once a card's sheet has been
# opened and closed, instead of the focus request landing on a stale sink
# handle and stranding every keystroke on <body>.
#
# Every chord below fires the instant a screen switch or a sheet close
# hands focus back to the pane — a spawned future — so every keystroke is
# paced (harness.sh `e2e_key_paced`). An earlier version retried each
# leg's navigation until a screenshot pixel said the table had left the
# glass; the vault is the oracle now (adr/2026-08-headless-x11-e2e.md):
# each leg navigates once, paced, then types its marker sentence exactly
# once, and the day note either holds it or the leg fails loudly. The
# sentence is never retried — a retry after a slow-but-landed first send
# splices a duplicate into the buffer, and a substring grep would still
# print "ok" over the garbled result.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# The keyboard-only way onto a card's sheet with no extra file written: the
# Ctrl+P palette's "open loops" command, Down then Enter on its second
# row — permanent/missing-type.typ, rank 1, the first row that actually
# opens a sheet (rank 0, missing-meta, has no id row and answers a notice
# instead; adr/2026-09-loop-lines-open-their-notes.md). Shift+Escape is
# the one gesture that closes it — plain Escape never does
# (adr/2026-08-plain-escape-never-closes-the-sheet.md). Nothing is typed
# into the sheet, so this writes nothing to disk.
open_and_close_a_sheet() {
    e2e_key_paced ctrl+p
    e2e_type_paced "open loops"
    e2e_key_paced Return
    e2e_key_paced Down
    e2e_key_paced Return
    e2e_key_paced shift+Escape
}

e2e_key_paced ctrl+1

# --- leg 1: Ctrl+D, straight off the table-screen dead-end --------------
# Ctrl+D's own Enter arm is idempotent on disk (a note that already
# exists is just reactivated, no write), so chord-then-Enter is resent
# until the file exists — notice-resolves.test.sh's own idiom
leg1_reach() {
    open_and_close_a_sheet
    e2e_key_paced ctrl+d
    # the empty day offers "no note for <day> — press enter to start one"
    e2e_key_paced Return
    test -f "$E2E_DIR/vault/time/$E2E_TODAY.typ"
}
e2e_await leg1_reach \
    || e2e_fail "time/$E2E_TODAY.typ was never written after ctrl+d"
e2e_note_holds "time/$E2E_TODAY.typ" 'type: "daily"'

e2e_key_paced i
e2e_type "ctrl-d reaches the table pane after a sheet closes"
e2e_key Escape
e2e_note_holds "time/$E2E_TODAY.typ" \
    "ctrl-d reaches the table pane after a sheet closes"

# --- leg 2: Ctrl+2, its own pass through the same dead-end --------------
# Ctrl+D above already routed through go_logs (adr/2026-08-ctrl-b-recent-
# notes-picker.md: "landing on the daily means landing on the temporal
# screen too"), so this leg returns to the table first, the same as leg 3
# below. A dropped Ctrl+2 leaves the table pane on glass, where i and the
# sentence reach no note — and the assertion below fails.
e2e_key_paced ctrl+1
open_and_close_a_sheet
e2e_key_paced ctrl+2

e2e_key_paced i
e2e_type "ctrl-2 reaches the table pane after a sheet closes"
e2e_key Escape
e2e_note_holds "time/$E2E_TODAY.typ" \
    "ctrl-2 reaches the table pane after a sheet closes"

# --- leg 3: Ctrl+P, driving a command whose landing is disk-visible -----
# a command run by name proves the palette itself opened and dispatched,
# not just that the chord bound to it still fires
# (adr/2026-08-screen-switch-gesture.md — the same callback either way)
e2e_key_paced ctrl+1
open_and_close_a_sheet
e2e_key_paced ctrl+p
e2e_type_paced "go to logs"
e2e_key_paced Return

e2e_key_paced i
e2e_type "ctrl-p reaches the table pane after a sheet closes"
e2e_key Escape
e2e_note_holds "time/$E2E_TODAY.typ" \
    "ctrl-p reaches the table pane after a sheet closes"

echo "ok: $(basename "$0")"
