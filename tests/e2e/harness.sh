# The only tests that drive the shipped window.
#
# `make test` renders components through `dioxus_ssr` (adr/2026-07-ui-covered-at-100.md):
# that sees markup, never a WebKitGTK surface. Focus, layout, the compiled
# SVG and the modal caret live on the other side of that boundary, so this
# harness runs the real binary in a headless X server and types at it
# (adr/2026-08-headless-x11-e2e.md).
#
# Sourced by each tests/e2e/*.test.sh; never run on its own.

set -u

E2E_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
E2E_BIN="$E2E_ROOT/target/release/note-system"
# 60 x 0.5 s. The ceiling is the first typst compile of a fresh vault, not
# any single assertion: a save lands 500 ms after the last keystroke.
E2E_POLLS=60
E2E_POLL_SLEEP=0.5
# The app's clock, pinned through NOTE_TODAY
# (adr/2026-09-note-today-pins-the-clock-for-e2e.md): the fixture vault
# lives in July 2026 and ships no note for this day, so "today" is empty
# on every run, and the harness and the app agree on the date whatever
# the wall clock says — a scenario straddling midnight used to write
# time/<yesterday>.typ and poll for time/<today>.typ.
E2E_TODAY=2026-07-24

e2e_fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# Runs "$@" until it succeeds, never more than E2E_POLLS times. Every wait
# in this file goes through here: a fixed sleep is either a flake or a
# tax, and both get paid on every run.
e2e_await() {
    for _ in $(seq "$E2E_POLLS"); do
        "$@" >/dev/null 2>&1 && return 0
        sleep "$E2E_POLL_SLEEP"
    done
    return 1
}

e2e_wm_ready() {
    xprop -root _NET_SUPPORTING_WM_CHECK | grep -q window
}

# Empty output is a failure, so e2e_await can poll it.
e2e_window() {
    xdotool search --name "Dioxus App" | head -1 | grep .
}

# Named up front, so a missing tool fails the run in its first second and
# by its own name, not thirty seconds in as "the app opened no window".
e2e_preflight() {
    for tool in Xvfb i3 xprop xdotool import sqlite3 xclip; do
        command -v "$tool" >/dev/null 2>&1 || e2e_fail \
            "$tool is not installed; the e2e suite needs Xvfb, i3, xorg-xprop, xdotool, ImageMagick (import) and sqlite3"
    done
    test -x "$E2E_BIN" || e2e_fail "$E2E_BIN is not built; run make e2e"
}

# The caller installs the teardown: a scenario wants `trap e2e_stop EXIT`,
# a persona session wants the window to outlive the shell that opened it.
e2e_start() {
    e2e_preflight
    E2E_DIR=$(mktemp -d)

    cp -r "$E2E_ROOT/tests/fixtures/vault" "$E2E_DIR/vault"
    # the fixture ships a built index; a run proves the app rebuilds one
    rm -rf "$E2E_DIR/vault/.index"

    # -displayfd: Xvfb picks a free display and reports it, so two tests
    # running at once never fight over a hardcoded :99
    Xvfb -displayfd 3 -screen 0 1400x900x24 3>"$E2E_DIR/display" \
        >"$E2E_DIR/xvfb.log" 2>&1 &
    E2E_XVFB=$!
    e2e_await test -s "$E2E_DIR/display" \
        || e2e_fail "Xvfb reported no display: $(cat "$E2E_DIR/xvfb.log")"
    DISPLAY=":$(cat "$E2E_DIR/display")"
    export DISPLAY

    # Without a window manager xdotool cannot focus anything — it says so
    # ("your windowmanager claims not to support _NET_ACTIVE_WINDOW") and
    # every keystroke afterwards is delivered to the root window and lost.
    # The socket lives here, not in $XDG_RUNTIME_DIR, or a run collides
    # with the developer's own i3 and refuses to start.
    printf 'font pango:monospace 8\nnew_window none\nipc-socket %s/i3.sock\n' \
        "$E2E_DIR" > "$E2E_DIR/i3.conf"
    i3 -c "$E2E_DIR/i3.conf" >"$E2E_DIR/i3.log" 2>&1 &
    E2E_WM=$!
    e2e_await e2e_wm_ready \
        || e2e_fail "no window manager: $(cat "$E2E_DIR/i3.log")"

    # Xvfb has no GPU: the dmabuf renderer WebKitGTK prefers cannot
    # allocate against it, and compositing falls back to a blank surface.
    WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    WEBKIT_DISABLE_DMABUF_RENDERER=1 \
    LIBGL_ALWAYS_SOFTWARE=1 \
    NOTE_TODAY="$E2E_TODAY" \
    NOTE_VAULT="$E2E_DIR/vault" "$E2E_BIN" >"$E2E_DIR/app.log" 2>&1 &
    E2E_APP=$!
    e2e_await e2e_window \
        || e2e_fail "the app opened no window: $(cat "$E2E_DIR/app.log")"
    xdotool windowactivate --sync "$(e2e_window)" \
        || e2e_fail "the window refused focus"
}

# Runs from the EXIT trap, so a failed assertion still tears the display
# down: a leaked Xvfb holds its display number and a leaked i3 holds its
# socket, and both poison the next run.
e2e_stop() {
    status=$?
    kill "${E2E_APP:-}" "${E2E_WM:-}" "${E2E_XVFB:-}" 2>/dev/null || true
    if [ "$status" -eq 0 ]; then
        rm -rf "$E2E_DIR"
    else
        echo "--- app log ---" >&2
        cat "$E2E_DIR/app.log" >&2
        echo "--- kept for inspection: $E2E_DIR" >&2
    fi
}

e2e_key() {
    xdotool key --clearmodifiers "$@"
}

# The editor is modal (adr/2026-08-caret-shape-is-the-mode-indicator.md):
# typing prose means pressing i first, exactly as a person would.
e2e_type() {
    xdotool type --delay 40 "$1"
}

# Screenshots are for a person or a persona to look at, never asserted
# on (adr/2026-08-headless-x11-e2e.md) — and for pacing: `import`'s round
# trip through the compositor is real wall-clock work that only returns
# once the window has something to hand back.
e2e_shot() {
    import -window root "$1"
}

# `e2e_key`/`e2e_type` return the instant xdotool's XTest calls do, not
# once WebKitGTK has drained the event. Nothing needs pacing any more:
# the sink holds the focus for the life of the window and reads every
# key against the state the previous key produced
# (adr/2026-09-the-sink-is-the-one-keyboard-socket.md), so a burst typed
# right after a chord lands whole under any load. The paced helpers stay
# for the scenarios written with them — one screenshot round trip, the
# app's own speed, never a guessed sleep. Never retry a sentence typed
# into a note (a retry after a slow-but-landed first send splices a
# duplicate into the buffer); let `e2e_note_holds`' read-only poll absorb
# the autosave debounce.
e2e_key_paced() {
    e2e_key "$@"
    e2e_shot "$E2E_DIR/probe.png"
}
e2e_type_paced() {
    e2e_type "$1"
    e2e_shot "$E2E_DIR/probe.png"
}

# i3's "new_window none" leaves the single tiled window undecorated and
# filling the screen from (0, 0), so a root-relative click lands at the
# same coordinate a root screenshot would show it at.
e2e_click_at() {
    xdotool mousemove --sync "$1" "$2" click 1
}

# A press at ($1, $2), a move to ($3, $4) and a release there — the
# table's own drag. Two moves, not one: a lone --sync move can be
# coalesced with the press, and the pane needs a mousemove to read the
# gesture as a drag rather than a click.
e2e_drag_at() {
    xdotool mousemove --sync "$1" "$2"
    xdotool mousedown 1
    xdotool mousemove --sync "$(( ($1 + $3) / 2 ))" "$(( ($2 + $4) / 2 ))"
    xdotool mousemove --sync "$3" "$4"
    xdotool mouseup 1
    e2e_shot "$E2E_DIR/probe.png"
}

# The same drag with Shift held for the whole of it: on the table's void
# that is the marquee (adr/2026-09-shift-drag-selects-cards.md).
e2e_shift_drag_at() {
    xdotool keydown shift
    e2e_drag_at "$@"
    xdotool keyup shift
}

# The chrome header (assets/theme.css .chrome: flex, 8px/16px padding,
# 16px gap, align-items center, 20px min-height) packs two 14x14 icons
# left to right, table then logs, both centred on the header's 20px-tall
# content box — which is 20px, not 14px, because the header reserves the
# notice line's own line box in every state
# (adr/2026-09-the-table-draws-the-notice-line.md). Measured off an
# e2e_shot of a fresh window: table's icon spans x 16-29, logs' spans
# x 46-58, both y 12-24 — the coordinates below are each icon's centre.
e2e_click_chrome() {
    case "$1" in
        table) e2e_click_at 23 18 ;;
        logs) e2e_click_at 53 18 ;;
        *) e2e_fail "e2e_click_chrome: unknown icon '$1'" ;;
    esac
}

e2e_file_appears() {
    e2e_await test -f "$E2E_DIR/vault/$1" || e2e_fail "$1 was never written"
}

# The vault is the oracle. The app's own promise is that the file on disk
# trails the screen by at most the 500 ms idle timer
# (adr/2026-07-debounced-autosave.md), so what reached the note is a
# stronger and steadier verdict than what reached the pixels.
e2e_note_holds() {
    e2e_await grep -qF "$2" "$E2E_DIR/vault/$1" || e2e_fail \
        "$1 never held '$2'; it holds:
$(cat "$E2E_DIR/vault/$1" 2>/dev/null)"
}

# The index is the second oracle: a note on disk with no row under its
# own id is exactly what the app-indexes-its-own-writes and watcher
# scenarios pin (adr/2026-09-the-app-indexes-its-own-writes.md). The
# query must answer a lone `1` row. The file is checked before opening
# it so polling never creates an empty db ahead of the app's own schema
# write.
e2e_index_query() {
    test -f "$E2E_DIR/vault/.index/index.db" || return 1
    sqlite3 "$E2E_DIR/vault/.index/index.db" "$1" 2>/dev/null | grep -q '^1$'
}
e2e_index_holds() {
    e2e_await e2e_index_query "$1" \
        || e2e_fail "vault/.index/index.db never answered 1 to: $1"
}
