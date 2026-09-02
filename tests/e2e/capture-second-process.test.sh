#!/bin/sh
# `wl-paste | app --capture` is a headless second process
# (adr/2026-08-capture-headless-second-process.md): it reads the paste on
# stdin, writes one capture note, prints its path and exits 0 — and the
# running app finds out through its watcher, like any other outside
# change, so the new note's row lands in the index with no keystroke
# sent to the window. A run with no vault to write into refuses on stderr
# with exit 1 and writes nothing.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# the app's own startup survey must be over before the capture lands, or
# the row could be that survey's rather than the watcher's
e2e_index_holds "SELECT 1 FROM notes WHERE id = 'plain-files';"

path=$(printf 'captured by a second process\nsecond line of the paste\n' \
    | NOTE_VAULT="$E2E_DIR/vault" "$E2E_BIN" --capture) \
    || e2e_fail "app --capture exited non-zero"
case "$path" in
    "$E2E_DIR/vault/capture/capture-"*.typ) ;;
    *) e2e_fail "app --capture printed '$path', not a capture path" ;;
esac
grep -qF 'captured by a second process' "$path" \
    || e2e_fail "$path does not hold the paste"
grep -qF 'second line of the paste' "$path" \
    || e2e_fail "$path lost the paste's second line"

id=$(basename "$path" .typ)
e2e_index_holds \
    "SELECT 1 FROM notes WHERE id = '$id' AND category = 'capture';"

# no vault: the refusal is on stderr, the exit code is 1, nothing written
count_before=$(find "$E2E_DIR/vault/capture" -name '*.typ' | wc -l)
refusal=$(printf 'nowhere to go\n' \
    | env -u NOTE_VAULT -u HOME "$E2E_BIN" --capture 2>&1 >/dev/null)
status=$?
[ "$status" = 1 ] || e2e_fail "app --capture with no vault exited $status, not 1"
[ "$refusal" = "no vault: define NOTE_VAULT or HOME" ] \
    || e2e_fail "app --capture with no vault said '$refusal'"
[ "$(find "$E2E_DIR/vault/capture" -name '*.typ' | wc -l)" = "$count_before" ] \
    || e2e_fail "a refused capture still wrote a note"

echo "ok: $(basename "$0")"
