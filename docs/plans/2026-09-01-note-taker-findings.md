# Fix the note-taker session findings

## Goal
Every Broken and Confusing finding from the 2026-09-01 note-taker session is fixed or explicitly superseded, with a regression test pinning each one.

## Out of scope
Finding #5 (typing inside a `- [ ]` marker via `$` then `i`) — correct vim behavior, no change.
Finding #9 (plain Escape not closing a sheet) — documented design, `adr/2026-08-plain-escape-never-closes-the-sheet.md` stands.
Named registers, macros, marks, visual block, jumplist — the v2 ceiling is unchanged.

## Constraints
The index stays derived and rebuildable; plain `.typ` files remain the source of truth.
Fixes must match the standing ADRs: the watcher round-trips the app's own writes (`adr/2026-08-watcher-feeds-the-ui.md`), notices follow the severity ladder with Escape-acknowledgement and resolution-beats-gestures (`adr/2026-08-status-surface-owns-notices.md`), screen chords run the same callbacks as icons and palette (`adr/2026-08-screen-switch-gesture.md`), and a pane retakes focus whenever nothing else holds it (`adr/2026-08-the-pane-holds-focus.md`).
Making open-loops lines clickable supersedes the "nothing clickable" clause of `adr/2026-08-loops-list-overlay.md` and needs its own ADR recording why (v1 sheets removed the old objection); every decision made during this work gets an ADR.
Whenever Dioxus code is written or understood, read `.claude/dioxus.md` first.
No naked `.unwrap()`, no while loops, colours only in `assets/theme.css`, all UI strings English.
`make test` must hold 100% region/line/function coverage when done.
Window-level regressions get e2e scenarios under `tests/e2e/` (headless X11, asserting on `.typ` files and screen state), since `make test` cannot see focus, the watcher, or the WebKitGTK surface.

## Items
1. Index freshness (findings #1 and #8): diagnose why notes created in-app (`ctrl+n`, `ctrl+d`) never reach `vault/.index/index.db` despite the watcher round-trip promise, fix the pipeline so link-following (`gf`, `ctrl+enter`), the recent-notes picker, card sheets, and dangling-link detection all see a note created seconds ago.
2. Notice lifecycle (finding #2): the "sheet: no note has the id" notice obeys the status ADR — Escape at the bottom of the escape ladder acknowledges it, a later successful resolution clears it, and it never outlives its cause across unrelated navigation.
3. Table screen keyboard reachability (findings #3 and #6): with no sheet open, `ctrl+2`, `ctrl+d`, and `ctrl+p` work from the table screen — the table pane holds focus the way the logs pane does, so no screen is a keyboard dead-end.
4. Focus after mouse navigation (finding #4): after clicking a chrome icon or any mouse-driven screen switch, typed keystrokes are never silently swallowed — the receiving pane retakes focus, and prose typed immediately after a switch reaches the note and the disk.
5. Open-loops lines open their notes (finding #7): clicking a loop line — and a keyboard path to the same — opens the note that owes the debt (sheet or editor as the note's category dictates), with a new ADR superseding the inert-lines clause now that v1 sheets exist.
6. E2e regression scenarios: one `tests/e2e/*.test.sh` scenario per fixed finding (create-then-follow, notice acknowledge/resolve, table-screen chords, type-after-mouse-switch, loop-line-opens-note), and a full `make e2e` pass over old and new scenarios.

## Acceptance
`make static && make test` green with coverage at 100%.
`make e2e` green, including the new scenarios, each of which fails against the pre-fix binary's behavior.
Every finding from the session report is either fixed with a pinned regression test or named out of scope above.
New ADRs exist for the loops-click decision and any other decision made along the way.

## Check
`make static && make test`
