#!/bin/sh
# A loop line opens the note that owes the debt (finding #7,
# adr/2026-09-loop-lines-open-their-notes.md): Enter on a highlighted row
# and a direct click both call `open_loop`, which opens the note by its
# own path — into the same sheet a card click would open — while a click
# on the overlay itself (its own "backdrop", the box's own onclick) still
# only closes it.
#
# Pre-fix the loops list is inert (adr/2026-08-loops-list-overlay.md, the
# clause this ADR supersedes): Enter and a row click only closed the
# overlay, so the sentences typed below would land nowhere.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# permanent/missing-meta.typ ("No meta call at all") sorts first
# alphabetically and is typeless, so it is rank 0. It carries no #meta at
# all, so the index has no id row for it — routing by id would answer None
# and open nothing. This is exactly the family the path-carrying fix
# exists for: `open_loop` does `Editor::open(root.join(path))` with no
# lookup, so the note opens from its path alone. This scenario's keyboard
# leg targets rank 0 directly, pinning that family at the window level.
MISSING_META="permanent/missing-meta.typ"
# permanent/missing-type.typ carries #meta(id: "missing-type", ...) but no
# type field — also src/loops.rs's typeless family, rank 1. The mouse leg
# targets it: a note the index CAN resolve, opened by path all the same.
MISSING_TYPE="permanent/missing-type.typ"

# Every overlay opened below, and every sheet closed, grabs or releases
# focus through its own async onmounted, so every keystroke is paced
# (harness.sh `e2e_key_paced`). A retry of the typed sentence is ruled
# out twice over: a retry that fires after a slow-but-landed first send
# splices a duplicate into the open buffer, and here it can also open a
# save conflict (the disk changing under an editor a fresh retry already
# reopened, `save: the note changed on disk`), which swallows the
# sentence entirely. So every leg below sends its whole "open loops ->
# reach the row -> type" chain exactly once, paced at every step, and
# `e2e_note_holds` supplies the only retry — a read-only grep absorbing
# the debounced autosave, never resending a keystroke.

# --- keyboard leg: Ctrl+P palette -> "open loops" -> Enter on rank 0 ----
# no Down: rank 0 is missing-meta, the note with no #meta at all and so no
# id row — it opens only because open_loop uses the path, never a lookup
e2e_key_paced ctrl+p
e2e_type_paced "open loops"
e2e_key_paced Return
e2e_key_paced Return
e2e_key_paced i
e2e_type "keyboard opened this loop line"
e2e_key Escape
e2e_note_holds "$MISSING_META" "keyboard opened this loop line"

# leave the note and close the sheet, back to a bare table pane
# (adr/2026-08-shift-escape-leaves-the-note.md)
e2e_key_paced Escape
e2e_key_paced shift+Escape

# --- mouse leg: reopen the overlay, click the row directly -------------
# row coordinates measured off an e2e_shot of this exact overlay: the box
# is fixed at top:96px, centred, 480px wide (assets/theme.css
# .command-palette); "missing-type · typeless" is its second line, rank 1
e2e_key_paced ctrl+p
e2e_type_paced "open loops"
e2e_key_paced Return
e2e_click_at 520 171
e2e_shot "$E2E_DIR/probe.png"
e2e_key_paced i
e2e_type "mouse click opened this loop line"
e2e_key Escape
e2e_note_holds "$MISSING_TYPE" "mouse click opened this loop line"

e2e_key_paced Escape
e2e_key_paced shift+Escape

# --- backdrop leg: a click on the overlay's own box only closes it -----
# today's daily note is the file this leg proves receives the keystrokes
# instead — table-screen reachability is finding #3's fix
# (adr/2026-08-screen-switch-gesture.md)
e2e_open_todays_daily() {
    e2e_key ctrl+d
    e2e_key Return
    test -f "$E2E_DIR/vault/time/$E2E_TODAY.typ"
}
e2e_await e2e_open_todays_daily \
    || e2e_fail "time/$E2E_TODAY.typ was never written"

# the "OPEN LOOPS" head sits inside the same box as every row but wears
# no row's own onclick, so a click here bubbles to the box's onclick and
# only closes the overlay (adr/2026-09-loop-lines-open-their-notes.md)
e2e_key_paced ctrl+p
e2e_type_paced "open loops"
e2e_key_paced Return
e2e_click_at 520 110
e2e_shot "$E2E_DIR/probe.png"
e2e_key_paced i
e2e_type "backdrop click left the day note in focus"
e2e_key Escape
e2e_note_holds "time/$E2E_TODAY.typ" "backdrop click left the day note in focus"

if grep -qF "backdrop click left the day note in focus" \
    "$E2E_DIR/vault/$MISSING_TYPE" 2>/dev/null; then
    e2e_fail "the backdrop click reopened the loop's note instead of just closing"
fi

echo "ok: $(basename "$0")"
