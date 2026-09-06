#!/bin/sh
# Cards never overlap (adr/2026-09-cards-yield-on-drop.md): a card that
# lands holds the place it was given, and every card it covers slides
# clear. Two notes created in a row land on the same viewport centre, so
# the second one's landing pushes the first — and the push reaches the
# positions file through the store's own debounce, the one seam every
# placement uses.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

E2E_POSITIONS="$E2E_DIR/vault/.index/positions"

# Every pair of persisted placements, a card's width and height apart:
# 184 is CARD_WIDTH + CARD_GAP and 64 is CARD_HEIGHT + CARD_GAP, the two
# clearances src/table.rs resolves a drop against.
e2e_positions_clear() {
    test -f "$E2E_POSITIONS" || return 1
    awk '
        { x[NR] = $2; y[NR] = $3; n = NR }
        END {
            for (i = 1; i <= n; i++)
                for (j = i + 1; j <= n; j++) {
                    dx = x[i] - x[j]; if (dx < 0) dx = -dx
                    dy = y[i] - y[j]; if (dy < 0) dy = -dy
                    if (dx < 184 && dy < 64) exit 1
                }
        }
    ' "$E2E_POSITIONS"
}

e2e_yielded() {
    grep -q '^yield-alpha ' "$E2E_POSITIONS"
}

e2e_create_concept() {
    e2e_key_paced ctrl+n
    e2e_type_paced "concept"
    e2e_key_paced Return
    e2e_type_paced "$1"
    e2e_key_paced Return
}

e2e_create_concept "yield alpha"
e2e_file_appears "permanent/yield-alpha.typ"

# the second card lands on the first one's birth slot, to the pixel
e2e_create_concept "yield beta"
e2e_file_appears "permanent/yield-beta.typ"

e2e_await e2e_yielded || e2e_fail \
    "the covered card never yielded into the positions file; it holds:
$(cat "$E2E_POSITIONS" 2>/dev/null)"

e2e_positions_clear || e2e_fail \
    "two placed cards still overlap; the file holds:
$(cat "$E2E_POSITIONS" 2>/dev/null)"

echo "ok: $(basename "$0")"
