#!/bin/sh
# Shift+drag selects cards, and the set moves as one body
# (adr/2026-09-shift-drag-selects-cards.md). A marquee drawn over the
# first row's two leftmost cards picks both; dragging either one then
# takes the other with it, keeping its offset to the pixel — which the
# positions file, the one oracle here, records as two entries a card
# pitch apart on the same row.
#
# The geometry is the origin grid's own (src/table.rs `fallback_slot`):
# unplaced notes stack four to a row at x 32, 224, 416, 608 and y 32,
# 128, ..., each card 176 x 56 canvas units. The screen coordinates below
# are those plus the chrome's height, which the app measures for itself
# from the press — so the band is drawn generously in y (it must cover
# row 0 and miss row 1 whatever the header came out to) and tightly in x
# (it must reach column 1 at 224 and miss column 2 at 416).
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

E2E_POSITIONS="$E2E_DIR/vault/.index/positions"

# Exactly two placements, both on the same row, one grid pitch apart:
# 192 is CARD_WIDTH + GRID_GAP, the columns the two picked cards started
# in. Their y is 400 below the row they were dragged off.
e2e_moved_together() {
    test -f "$E2E_POSITIONS" || return 1
    awk '
        { x[NR] = $2; y[NR] = $3; n = NR }
        END {
            if (n != 2) exit 1
            if (y[1] != 432 || y[2] != 432) exit 1
            dx = x[1] - x[2]; if (dx < 0) dx = -dx
            if (dx != 192) exit 1
            if (x[1] != 32 && x[1] != 224) exit 1
        }
    ' "$E2E_POSITIONS"
}

e2e_click_chrome table
e2e_shot "$E2E_DIR/probe.png"

# the band: from the void left of column 0 and below row 0, up and right
# across the first two cards of row 0
e2e_shift_drag_at 5 140 300 60

# and the drag: 400 straight down, taken on the card in column 0
e2e_drag_at 100 85 100 485

e2e_await e2e_moved_together || e2e_fail \
    "the picked pair did not move together; the file holds:
$(cat "$E2E_POSITIONS" 2>/dev/null)"

echo "ok: $(basename "$0")"
