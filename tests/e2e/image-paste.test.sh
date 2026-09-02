#!/bin/sh
# An image on the clipboard pastes into the vault
# (adr/2026-09-an-image-pastes-into-assets.md): with no text to paste, p
# writes the clipboard's png under assets/ and spells the #image call
# that shows it. xclip serves a one-pixel png on the display's clipboard;
# the assets/ file and the note are the oracles.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start

# a 1×1 red pixel, the smallest png there is; xclip forks and keeps
# serving the selection until the display goes
printf 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==' \
    | base64 -d > "$E2E_DIR/pixel.png"
xclip -selection clipboard -t image/png -i "$E2E_DIR/pixel.png"

# today's empty day: enter creates it, and the note opens on its last line
e2e_reach_todays_daily() {
    e2e_key_paced ctrl+d
    e2e_key_paced Return
    test -f "$E2E_DIR/vault/time/$E2E_TODAY.typ"
}
e2e_await e2e_reach_todays_daily \
    || e2e_fail "time/$E2E_TODAY.typ was never written"

# normal-mode p: the clipboard holds no text, so the image is what pastes
e2e_key_paced p
e2e_await sh -c "ls \"$E2E_DIR\"/vault/assets/$E2E_TODAY-*.png" \
    || e2e_fail "no png landed under assets/ for the pasted image"
e2e_note_holds "time/$E2E_TODAY.typ" "#image(\"/assets/$E2E_TODAY-"
# the file is what the call names, byte for byte the png xclip served
name=$(basename "$(ls "$E2E_DIR"/vault/assets/"$E2E_TODAY"-*.png | head -1)")
grep -qF "#image(\"/assets/$name\")" "$E2E_DIR/vault/time/$E2E_TODAY.typ" \
    || e2e_fail "the note names a file other than $name"
head -c 8 "$E2E_DIR/vault/assets/$name" | od -An -tx1 | grep -q '89 50 4e 47' \
    || e2e_fail "assets/$name is not a png"

echo "ok: $(basename "$0")"
