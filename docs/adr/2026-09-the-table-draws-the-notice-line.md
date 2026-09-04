# The table draws the notice line

## Context

`adr/2026-09-the-status-surface-is-the-only-log.md` decided there is no
log file and no logging crate: "the status surface is the windowed app's
log".
That sentence was only true where the line was rendered, and
`grep -n 'class: "notice'` over `src/ui.rs` found exactly two sites — the
logs' centre column and the inside of the sheet.
A bare table, no sheet open, drew no notice line at all.
So every table-side failure that reports on the status surface —
`Source::Index` behind a switcher or a filter, `Source::Positions` on a
refused placements write, `Source::Delete` — had nowhere to speak, and the
user saw the effect (an empty list, a card that did not move) with no
cause.

The Ctrl+O work named the gap without closing it.
`adr/2026-09-ctrl-o-is-the-one-note-switcher.md` promises that "a broken
index costs the typed half only … the switcher still opens on it with the
notice on the status line saying why the rest is empty" — and on the bare
table there was no status line to say it on.
The three sibling tests in `src/ui.rs`
(`filter_cards_resolves_a_standing_index_notice`,
`the_switcher_resolves_a_standing_index_notice`,
`insert_link_resolves_a_standing_index_notice`) all open a sheet first,
and one of them says why in its own doc comment: "a sheet … is the
vehicle: it keeps a place to render the notice line".
The tests were working around the defect.

## Decision

**The chrome draws the notice line for the bare table.**
`Chrome` gains one prop, `notice: Option<Notice>`, and renders the same
node the other two sites render — `p { class: "notice {shown.class()}",
"{shown.text}" }`, class and text unchanged — between the filter label and
the ember.
The chrome is the table's only chrome, it already carries the loops ember
and the liveness glyph, and it is the one header both screens mount.

**The caller decides, so no screen ever shows two.**
The one call site passes the notice only when
`screen() == Screen::Table && sheet_open.is_none()`, and `None`
otherwise: the logs draw it in their centre column, a sheet draws it
inside itself, and the chrome takes it exactly when neither does.
Nothing in `src/status.rs` changes — same severity ladder, same
source-based replacement, same resolve-on-a-good-read rule
(`adr/2026-09-index-notices-resolve-on-a-good-lookup.md`).
This is a third place to *render* the one line, not a second line.

**The header reserves the line's height in every state.**
`.chrome` gains `min-height: 20px`, so its content row is as tall as the
notice's line box whether a notice stands or not, and the canvas below
never moves when one arrives or leaves (AIR LAY: nothing on screen moves
unless the user moved it).
The header's total height goes from a constant 30px to a constant 36px;
the 14×14 icons stay centred in it, measured at y 12–24 and recorded in
`tests/e2e/harness.sh`, whose `e2e_click_chrome` now aims at the new
centre.
`.chrome .notice` drops the line's 16px padding, pins `line-height` to
20px and truncates with an ellipsis rather than wrapping — a second line
would grow the header and shift the canvas, which is the thing being
prevented.
The full text of a truncated message stays readable in the palette's
"notices" history, which already holds every notice ever reported.

## Rejected

- **A notice only inside the overlays that cause one.** It puts the
  message where the failure was raised rather than where the app's one
  status surface is, and it multiplies the render sites by the number of
  overlays; a `Source::Positions` failure, which has no overlay at all,
  would still have nowhere to go. The status ADR's whole point is one
  surface, one owner.
- **A modal error dialog on the table.** It blocks, and the app has no
  hard blocks; it also fails the severity ladder — an `Info` "captured
  c-1" is not a dialog, and a ladder whose top rung is a different
  mechanism is two mechanisms.
- **A floating toast over the canvas.** It would not move the layout, but
  it either auto-dismisses (which AIR forbids for anything that matters,
  and which the `Status` ladder already refuses: a warning stands until
  resolved or acknowledged) or it occludes cards while it stands. The
  chrome is already reserved space nobody reads a card in.
- **Leaving it as the sheet's job and telling the user to open one.** The
  failures worth reporting here are exactly the ones that keep a sheet
  from opening.

## Consequence

`make test` keeps 100%: three new ui tests pin the behaviour — a failed
index read on the bare table shows the notice with its text in the chrome,
the next good read resolves it, and a sheet opening over it takes the line
with it (one `notice-warning` on the page, none left in the chrome).
`tests/e2e/table-notice.test.sh` drives the shipped window: the harness
never asserts on pixels (`adr/2026-08-headless-x11-e2e.md`), so it proves
what only the window can — that the taller header costs the table nothing,
the switcher still opens over a squatted index, and the same query lands
once the index is readable again.
