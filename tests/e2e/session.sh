#!/bin/sh
# A persistent app session for a persona to poke at by hand
# (adr/2026-08-headless-x11-e2e.md). Same launch as the automated
# scenarios — same disposable vault copy, same headless display — minus
# the teardown trap, so the window outlives the shell that opened it.
#
#   tests/e2e/session.sh start writer   # prints DISPLAY and VAULT
#   tests/e2e/session.sh stop  writer
#
# The name isolates concurrent personas: one display, one vault and one
# window each, so nobody's keystrokes land in anybody else's note.
. "$(dirname "$0")/harness.sh"

name=${2:-default}
state="/tmp/note-system-session-$name"

case "${1:-start}" in
    start)
        [ -f "$state" ] && e2e_fail "session '$name' is already open ($(cat "$state"))"
        e2e_start
        printf 'DISPLAY=%s VAULT=%s LOG=%s PIDS=%s,%s,%s\n' \
            "$DISPLAY" "$E2E_DIR/vault" "$E2E_DIR/app.log" \
            "$E2E_APP" "$E2E_WM" "$E2E_XVFB" > "$state"
        cat "$state"
        ;;
    stop)
        [ -f "$state" ] || e2e_fail "no session '$name' is open"
        pids=$(sed 's/.*PIDS=//' "$state" | tr ',' ' ')
        # shellcheck disable=SC2086
        kill $pids 2>/dev/null || true
        rm -f "$state"
        echo "session '$name' closed"
        ;;
    *)
        e2e_fail "usage: session.sh start|stop [name]"
        ;;
esac
