# A loop line opens the note that owes it

## Context

`adr/2026-08-loops-list-overlay.md` refused to make loop lines clickable:
"a typeless permanent note has nowhere to open until v1's table, so two
thirds of the list would be inert and the grammar would be inconsistent."
That objection is gone. v1's table and its sheets now exist — every note
the loops list can name (a typeless permanent note, a dangling link's
source, an unsummarized capture, a note the index could not read cleanly)
opens the same way a card click does: into the sheet. Unlike a card click,
a loop line names a note by the one thing every family already has — its
path — rather than by an id the index may hold no row for at all, so
opening it needs no lookup, and no failure notice, to reach: it is exactly
as reliable as `open_sheet_note`'s own success path, with the index read
removed from the middle of it.

The 2026-09-01 note-taker session flagged this directly (finding #7): a
typeless permanent note surfaced only in the loops list had no way to
reach its sheet at all short of scrolling the table by eye.

## Decision

- `loops::lines` returns `Vec<LoopLine>` instead of `Vec<String>`. Each
  line carries `text` (byte-for-byte what it always rendered) and `path` —
  the path of the note that **owes** the debt. For a dangling link that is
  `link.source`, never the missing target: the source is the note with
  something to fix, and it is the only one of the two guaranteed to exist.
- A loop line's click or Enter calls `open_loop`, a new callback that opens
  the note directly by that path — it does **not** route through
  `open_id`, `gf`'s and Ctrl+Enter's shared entry point. `open_id`
  resolves an id through the index and, by design, stays inert on one the
  table does not know (`adr/2026-08-permanent-links-open-sheets.md`:
  dangling links stay inert). A loop line can name a note the index has no
  id row for at all — one with no `#meta` call, or a broken one — so
  gating it the same way `open_id` gates a dangling link would make
  exactly the notes this overlay exists to surface the ones it cannot
  open. Carrying the path and opening it directly sidesteps the index for
  this one entry point instead of teaching `open_id` a second reachability
  rule that only it would use.
- **The destination still follows the category rule.** The path's leading
  directory (`dir_category`, `NoteCategory::from_dir`'s verdict) decides:
  a `time/` note lands on the logs through `select`, where its rail,
  calendar and crumbs are — a sheet over the table would float it with no
  card behind it, no rail and no calendar. Everything else opens
  `Editor::open(root.join(path))` into `show_sheet`, the same sheet a card
  click mounts. Time notes reach this list by three families (typeless,
  dangling source, anomalous — `dangling_links` has no category filter and
  `typeless_notes` excludes only Capture), so the time branch is a path
  users actually hit, not an hypothetical.
- **The logs can show a time note the rail excludes.** `select` asks the
  rail whether the id exists, and a typeless or id-less time note is
  loops debt the rail deliberately omits — but the file is the truth
  (`open_selected` falls back to opening the file on disk when the rail
  says no). The scale comes from the stem itself (`logs::scale_of_id`,
  the same parsers the rail sorts by), so no index row is needed at any
  point on the way to the logs. The one time file the logs cannot place —
  a stem no scale can parse — falls back to the sheet, the only surface
  that can show any file. A refused flush keeps everything where it was:
  the screen only switches once the selection really landed, `open_id`'s
  own guard.
- Mouse and keyboard reach a row identically, the Ctrl+L picker's own
  grammar over a list with no query field to carry it: arrows move a
  highlighted rank (clamped, no wrap, reset to 0 on open), Enter opens the
  highlighted row, a row's own click opens that row (`stop_propagation`,
  so it does not also fire the container's backdrop-close), and the
  container's click and Escape still close the overlay. Rows key on rank,
  not text — two debts can print an identical line.
- The list is not frozen at open the way the other pickers freeze their
  entries: `loops` is read live, exactly as it always was, so a resolved
  debt disappears from underneath the user while the overlay is still up
  — visible proof the fix landed. A highlighted rank can therefore point
  past a list that just shrank; `Enter` over that stale rank finds nothing
  and does nothing, the same shape as the Ctrl+L picker's "no matches"
  no-op.

## Rejected

- **Parsing the display string back apart** — `"linky → ghost · dangling"`
  already names the source, but recovering even its stem means re-deriving
  the same arrow-and-middle-dot grammar `loops::lines` just built, in the
  one place (`ui.rs`) that has no business knowing it — and a stem is not
  the path, which is what opening the note actually needs. Carrying the
  path alongside the text costs one field and removes an entire
  inverse-parser from the surface area.
- **One destination for every category** — the first version opened every
  loop line into the sheet, on the claim that no time note ever reached
  this list. The claim was false (a dangling link in a daily, a typeless
  time note, and an anomalous time file all reach it), and the result was
  a daily note drawn in a card sheet over the table with no card behind
  it. The category rule above replaced it.
- **Routing loop lines through `open_id`, the same entry point `gf` and
  Ctrl+Enter share** — tried first, and reverted: `open_id` is gated on
  `table_notes` reachability so a dangling link's *target* stays inert
  (`adr/2026-08-permanent-links-open-sheets.md`), and a note with no
  `#meta` at all has no id row to pass that gate either, even though it is
  exactly the kind of note this list exists to reach. Teaching `open_id` a
  second reachability rule just for loop lines would either weaken the
  dangling-link guard `gf` depends on or leave the malformed-meta family
  unreachable regardless; opening by path in `open_loop` avoids the
  conflict entirely rather than resolving it in `open_id`.
- **Freezing the list at open, like the other pickers** — those freeze
  because their query field would otherwise be filtering against text that
  moves under the caret mid-keystroke. Nothing here types into anything;
  freezing would only hide that a debt got resolved while the user was
  looking at it, which is the opposite of what a debt list is for.
