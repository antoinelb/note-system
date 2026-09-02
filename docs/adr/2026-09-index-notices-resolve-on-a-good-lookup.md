# Index notices resolve on a good lookup

## Context

The 2026-09-01 note-taker session (finding #2) opened a sheet on an id the
index had no row for and got the notice
`sheet: no note has the id …` — correctly reported as
`Notice::index(message)`, a `Warning` on `Source::Index`
(`adr/2026-08-status-surface-owns-notices.md`). Navigating away and back to
a real card left the same notice standing on the line, unrelated to
whatever the user was now looking at.

`grep -n "status.write().resolve"` before this change showed resolves for
`Source::Watcher`, `Source::Save`, `Source::Positions`, `Source::Conflict`
and `Source::Clipboard` — every source that reports trouble except
`Source::Index`. The five `Notice::index` reporters (`open_sheet`,
`open_filter`, `open_templates`, `open_jump`, `open_picker` — each an
index read on the way to a sheet or an overlay) had an `Err` arm that
reported but no `Ok` arm that resolved. The status ADR's own rule —
"resolution beats gestures: a later clean save clears its own failed-save
critical" — was never wired for this source, so the notice's only way off
the line was Escape or being replaced by an unrelated later notice.

## Decision

Every `Notice::index` reporter's matching successful lookup calls
`status.write().resolve(Source::Index)` on its `Ok` arm, gated on
`status.peek().has(Source::Index)` — the same write gate every other
source's resolve already uses (`status.rs`: "the callers' write gate, so a
tick with nothing to change writes nothing"), so a lookup with no notice
standing does not dirty the signal for nothing: `open_sheet_note` inside
`open_sheet`, `tag_names` inside `open_filter`, `template_names` inside
`open_templates`, and the two `completions` calls inside `open_jump` and
`open_picker`. A read that finds what it was looking for is proof the
condition the notice reported no longer holds.

Nothing that is not an index read resolves `Source::Index`. In particular
`show_sheet` — the landing half both a successful and a failed
`open_sheet_note` funnel through — does not itself call resolve.

`open_sheet` is the one reporter where the lookup's own success is not
enough: a lookup that finds the id can still lose the navigation to
`show_sheet`'s flush guard (an unsaved sheet whose own save fails keeps
the prior sheet standing). Resolving on the lookup's `Ok` arm before
`show_sheet` runs would clear the notice on a navigation that never
actually happened. So `open_sheet` calls `show_sheet` first and only then
resolves, and only when `sheet` now reads back the id just looked up —
proof `show_sheet` really landed, not just that the lookup succeeded. The
other four reporters (`open_filter`, `open_templates`, `open_jump`,
`open_picker`) have no such landing half to wait on — nothing downstream
of their lookup can refuse — so their `Ok` arm resolves immediately, still
before acting on the rest of the result.

`close_sheet` also resolves `Source::Index`, gated the same way, when the
sheet being left is itself the closed editor a failed lookup produced
(`editor.peek().note().is_none()`) — the one case where the notice's own
cause ends with no later index read to prove it: leaving the error sheet
(Shift+Escape, or `go_logs`'s own hygiene closing a sheet left open behind
the logs) is exactly as much proof the "no note has the id" condition no
longer applies to what is on screen as a fresh successful lookup would be.
A sheet left open behind a `go_table` screen switch is unaffected — the
sheet still renders, so its notice's cause has not ended.

Escape stays the manual out the status ADR already gives every warning:
both escape ladders' bottom rungs (`ui.rs`'s logs arm and the table pane's
sheet arm) call `status.write().acknowledge()` unconditionally whenever a
notice stands, with no `Source::Index` special case — confirmed by
reading both sites, not changed by this work.

## Rejected

- **Dropping the severity to `Info`** — an info notice yields to *any*
  later notice regardless of source, which would make an unrelated
  capture or save silently erase a message the user has not yet read.
  The notice is correctly a `Warning`: it should persist until the
  specific condition it names is gone, not until anything else happens.
- **Special-casing the sheet's id** — resolving only when the *same* id
  that failed later succeeds would leave the notice standing after,
  say, opening the filter overlay or the jump picker successfully, even
  though those are equally proof the index is readable again. The fix
  is per-source (any successful index read clears it), not per-id. This is
  distinct from `open_sheet`'s landing check above: that check asks
  whether *this* navigation reached the pane at all, not whether it named
  the same id an earlier failure did.

## Consequence

A successful index read is now a resolution, matching every other source
in `status.rs`. The two sibling failure tests
(`an_unopenable_index_surfaces_in_the_sheet`,
`a_sheet_over_a_missing_notes_table_reports_the_lookup_error`) are
untouched — they never reach an `Ok` arm, so nothing about them changes.

## Amendment: the retry is a lookup too (2026-09-02)

`open_sheet`'s same-id early return used to fire before the lookup, so
after a failed lookup left its bare sheet standing, clicking the same
card again — the natural retry once the index heals — was a complete
no-op: no re-lookup, no resolution, the error sheet and its notice both
stood. The guard now swallows a repeat only when the sheet is showing
this id **and** the editor actually holds its note; a bare error sheet
(closed editor) lets the click through, and the retried lookup is then
exactly the successful read that resolves the notice.
