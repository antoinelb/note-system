#!/bin/sh
# The bare table carries the status surface's one line
# (adr/2026-09-the-table-draws-the-notice-line.md): with no sheet open the
# table has no reading column, and until this change it drew no notice at
# all — a failed index read behind the Ctrl+O switcher's typed rows left
# an empty list and no reason on screen.
#
# What this scenario can and cannot prove. The harness never asserts on
# pixels (adr/2026-08-headless-x11-e2e.md), so the notice's own text is
# checked by the three ui tests in src/ui.rs, not here. What only the
# shipped window can show is that the chrome's new line does not cost the
# table anything: the app stays live through the failed read, the
# switcher still opens on it, and the same query works once the index is
# readable again — the leg that fails if the extra header row ever
# swallowed a keystroke or wedged the overlay.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# The app opens on the logs; the table icon is the left one in the chrome.
# The screen switch grabs focus through an async onmounted, so it is paced
# like any other (harness.sh e2e_key_paced).
e2e_click_chrome table
e2e_shot "$E2E_DIR/probe.png"

# The index must exist before it can be made unreadable.
e2e_index_holds "SELECT 1 FROM notes WHERE id = 'luhmann';"

# Squat the index by taking every permission off it: `Index::open` refuses,
# the switcher reports `links: …` on Source::Index, and the chrome draws
# it. Unlike replacing the file with a directory (the ui tests' sabotage),
# this leaves the rows intact, so lifting it needs no rebuild — and the
# watcher never sees it: `.index/` writes are filtered out before any
# batch (src/watch.rs).
chmod 000 "$E2E_DIR/vault/.index/index.db"

e2e_key_paced ctrl+o
e2e_type_paced "luhmann"
# nothing matched, so Enter is inert by design: no landing, no file
e2e_key_paced Return
e2e_key_paced Escape

# lift the squat; the next read is a good one and resolves the notice
chmod 644 "$E2E_DIR/vault/.index/index.db"

e2e_key_paced ctrl+o
e2e_type_paced "luhmann"
e2e_key_paced Return
e2e_key_paced i
e2e_type "the table stayed live through the failed read"
e2e_key Escape
e2e_note_holds "permanent/luhmann.typ" \
    "the table stayed live through the failed read"

echo "ok: $(basename "$0")"
