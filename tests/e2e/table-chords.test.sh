#!/bin/sh
# Table-screen chords after a sheet closes (findings #3/#6, fixed by the
# focus effect reading `sheet` and `screen`,
# adr/2026-09-sheet-and-screen-join-the-focus-effect.md): Ctrl+D, Ctrl+2
# and Ctrl+P all still reach the table pane once a card's sheet has been
# opened and closed, instead of the focus request landing on a stale sink
# handle and stranding every keystroke on <body>.
#
# Every chord below fires the instant a screen switch or a sheet close
# hands focus back to the pane — that handoff is a spawned future
# (adr/2026-09-sheet-and-screen-join-the-focus-effect.md), so the very
# first keystroke sent before it resolves can arrive early and land
# nowhere. A retry belongs only on the part of each leg that is genuinely
# idempotent: reopening and reclosing a sheet, resending a screen chord,
# retyping a palette query into a query box that is empty either way. It
# does NOT belong on "press i, type the marker sentence, press Escape" —
# an earlier version of this scenario wrapped that in the same retry as
# everything else, and empirically (two independent runs against the real
# harness) that is not side-effect-free: once in a while the first send
# actually lands, just slower than one grep check notices, and the retry
# fires `i` a second time onto a cursor sitting right after its own
# still-settling insertion, splicing a second copy of the sentence into
# the first. Because the assertions below are `grep -qF` substring checks,
# a run like that still prints "ok" while the note quietly holds garbled,
# duplicated text.
#
# So every leg below is split in two: a `_reach` half that only spends
# keystrokes idempotent from wherever the pane already stands (the sheet
# dance, the screen chord, the palette query) and is retried behind
# `e2e_await` until an observable, keyless proxy confirms the pane is
# listening; then, and only then, the marker sentence is typed exactly
# once, with no retry wrapping it. If that single send is ever the one
# that drops, the assertion below fails loudly instead of silently
# resending into a file that already, eventually, held the truth.
. "$(dirname "$0")/harness.sh"

trap e2e_stop EXIT INT TERM
e2e_start
today=$(date +%Y-%m-%d)

# `e2e_key`/`e2e_type` return the instant xdotool's XTest calls do, not
# once WebKitGTK has actually drained the event: a chain of chord, type,
# arrow and Enter fired back to back with nothing between them can queue
# faster than the pane processes them, which is a second, plainer way for
# a keystroke to land nowhere or land against a state that has already
# moved on — the same failure shape as the mount race itself, just from
# the sending side rather than the focus side. `e2e_shot` is the harness's
# own settle: unlike a guessed sleep, `import`'s round trip through the
# window compositor is real wall-clock work that only returns once the
# window has something to hand back, so it paces every send to the app's
# own speed instead of a duration this scenario would otherwise have to
# guess. notice-resolves.test.sh already leans on the same round trip to
# pace its own key-then-check steps. Every `_reach` half below is built
# only from these two, plus `open_and_close_a_sheet` — none of them ever
# writes a character into a note, so resending the whole half costs
# nothing when the pane was never listening in the first place.
e2e_key_paced() {
    e2e_key "$@"
    e2e_shot "$E2E_DIR/probe.png"
}
e2e_type_paced() {
    e2e_type "$1"
    e2e_shot "$E2E_DIR/probe.png"
}

# The keyboard-only way onto a card's sheet with no extra file written: the
# Ctrl+P palette's "open loops" command (the same overlay Ctrl+B's chord
# would open, adr/2026-08-ctrl-b-recent-notes-picker.md, but this one needs
# no prior visit history to be non-empty), Down then Enter on its second
# row. permanent/missing-meta.typ (rank 0, "no meta call at all") has no
# id row in the index and answers its own notice instead of opening a
# sheet (adr/2026-09-index-notices-resolve-on-a-good-lookup.md); rank 1,
# permanent/missing-type.typ, is the first row that actually opens one
# (adr/2026-09-loop-lines-open-their-notes.md). Shift+Escape is the one
# gesture that closes it — plain Escape never does
# (adr/2026-08-plain-escape-never-closes-the-sheet.md). Nothing is typed
# into the sheet, so this scaffold writes nothing to disk, and every step
# in it is safe to resend from wherever the pane already stands: reopening
# an already-closed sheet, or reopening the palette over one that never
# opened, both just redo the same idempotent navigation.
open_and_close_a_sheet() {
    e2e_key_paced ctrl+p
    e2e_type_paced "open loops"
    e2e_key_paced Return
    e2e_key_paced Down
    e2e_key_paced Return
    e2e_key_paced shift+Escape
}

# A keyless proxy for "the table screen's own card grid is no longer what
# is on glass" — reads the freshest probe screenshot a `_reach` half's own
# last `e2e_key_paced`/`e2e_type_paced` call already took, rather than
# spending a further round trip. (120, 100) sits inside the fixture
# vault's first card ("CLAIM · Atomic notes recombine better" —
# tests/fixtures/vault, a stock 1400x900 run, card fill ~rgb(118,114,134));
# the logs screen paints nothing there (~rgb(13,11,24), the plain
# background) and the command palette's own box is centred at 480px wide,
# nowhere near x=120, so a palette left open by a dropped Return neither
# hides nor fakes this pixel. The two readings are far enough apart (118
# vs 13) that 60 splits them with room on both sides — nothing like the
# 14x14, stroke-only chrome icons (assets/theme.css `.chrome svg.lit`),
# which would need exact anti-aliasing-aware coordinates to read reliably.
e2e_left_the_table() {
    bright=$(convert "$E2E_DIR/probe.png" -crop 1x1+120+100 \
        -format "%[fx:int(255*r)]" info: 2>/dev/null)
    [ -n "$bright" ] && [ "$bright" -lt 60 ]
}

e2e_key_paced ctrl+1

# --- leg 1: Ctrl+D, straight off the table-screen dead-end --------------
# The proxy here needs no pixel probe: Ctrl+D's own Enter arm is already
# idempotent on disk (src/ui.rs's `Key::Enter` arm — a note that already
# exists is just `reactivate()`d, no write), so resending chord-then-Enter
# until the file exists is exactly notice-resolves.test.sh's own
# `e2e_reach_todays_daily` idiom, safe for the same reason.
leg1_reach() {
    open_and_close_a_sheet
    e2e_key_paced ctrl+d
    # the empty day offers "no note for <day> — press enter to start one"
    e2e_key_paced Return
    test -f "$E2E_DIR/vault/time/$today.typ"
}
e2e_await leg1_reach \
    || e2e_fail "time/$today.typ was never written after ctrl+d"
e2e_note_holds "time/$today.typ" 'type: "daily"'

e2e_key_paced i
e2e_type "ctrl-d reaches the table pane after a sheet closes"
e2e_key Escape
e2e_note_holds "time/$today.typ" \
    "ctrl-d reaches the table pane after a sheet closes"

# --- leg 2: Ctrl+2, its own pass through the same dead-end --------------
# Ctrl+D above already routed through go_logs (adr/2026-08-ctrl-b-recent-
# notes-picker.md: "landing on the daily means landing on the temporal
# screen too"), so this leg returns to the table first, the same as leg 3
# below.
leg2_reach() {
    e2e_key_paced ctrl+1
    open_and_close_a_sheet
    e2e_key_paced ctrl+2
    e2e_left_the_table
}
e2e_await leg2_reach \
    || e2e_fail "ctrl+2 never left the table screen after a sheet closed"

e2e_key_paced i
e2e_type "ctrl-2 reaches the table pane after a sheet closes"
e2e_key Escape
e2e_note_holds "time/$today.typ" \
    "ctrl-2 reaches the table pane after a sheet closes"

# --- leg 3: Ctrl+P, driving a command whose landing is disk-visible -----
leg3_reach() {
    e2e_key_paced ctrl+1
    open_and_close_a_sheet
    e2e_key_paced ctrl+p
    # a command run by name proves the palette itself opened and
    # dispatched, not just that the chord bound to it still fires
    # (adr/2026-08-screen-switch-gesture.md — the same callback either
    # way); "go to logs" only lists while on the table, so a stray retry
    # that lands after an earlier attempt already reached the logs screen
    # finds no match, Return is a no-op over an unopened overlay, and
    # `e2e_left_the_table` below still answers true from the earlier
    # success
    e2e_type_paced "go to logs"
    e2e_key_paced Return
    e2e_left_the_table
}
e2e_await leg3_reach \
    || e2e_fail "ctrl+p's 'go to logs' command never left the table screen after a sheet closed"

e2e_key_paced i
e2e_type "ctrl-p reaches the table pane after a sheet closes"
e2e_key Escape
e2e_note_holds "time/$today.typ" \
    "ctrl-p reaches the table pane after a sheet closes"

echo "ok: $(basename "$0")"
