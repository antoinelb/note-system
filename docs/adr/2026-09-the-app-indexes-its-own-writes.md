# The app indexes its own writes

## Context

The 2026-09-01 note-taker session found notes created in-app (`ctrl+n`,
`ctrl+d`) absent from `vault/.index/index.db` right after creation:
link-following (`gf`, `ctrl+enter`), the recent-notes picker and
dangling-link detection could not see a note the session had just written.
`adr/2026-08-watcher-feeds-the-ui.md` already states the intent — "the
app's own writes round-trip through the watcher" — so this was read as a
regression in that round trip.

It is not. `tests/integration/watch.rs` gained two tests
(`creating_a_permanent_note_through_the_app_reaches_the_index`,
`capturing_through_the_app_reaches_the_index`) that call
`create::permanent` and `template::create_capture` — the exact functions
the UI calls — against a real `VaultWatcher`, and both pass on the
pre-fix binary. `notify` sees the write, `classify` attributes it, `apply`
indexes it. The watcher's round trip works.

What is missing is speed, not correctness. The round trip is `QUIET`
(200 ms debounce) plus however long the compute tier's survey lane takes
to run, and it starts only once notify's inotify event lands on the
debouncer's thread. A session that creates a note and immediately follows
a link, opens the recent picker, or checks for dangling links runs faster
than that — the note-taker's own keystrokes outran the watcher. The index
was never wrong for long; it was wrong for as long as the app chose to
depend on a filesystem notification round trip for a fact it already knew
the instant it wrote the file.

## Decision

The app's own write seams stop waiting on the watcher. Each one submits a
one-note `compute::Survey` job — `compute::touched` or `compute::removed`
— the instant it writes or deletes a file, carrying the same
vault-relative path and category the watcher's own `classify` would have
produced. Five seams do this: `create_note` (Ctrl+N), `create_time_note`
(Ctrl+D and the six relative-time commands), `capture_clipboard`
(Ctrl+Shift+V), `delete_note` (the sheet's delete and Ctrl+Shift+D), and
`undo_last`'s own `Intent::Delete` reversal (the palette's "undo delete
…") — a deleted note's return through `create_new` is exactly as much a
write the app just performed as the other four, and the same latency gap
would otherwise open on it: an undo followed immediately by `gf` back into
the restored note would outrun the watcher the same way a fresh
`ctrl+n` create used to. Each also invalidates the body cache for the same
path first, mirroring what the watcher drain already does for a
`Touched`/`Removed` change, so a sheet can never show a stale compile of a
note the app just wrote or removed.

The watcher is not touched and stays exactly what
`adr/2026-08-watcher-feeds-the-ui.md` describes: the channel for changes
that happen *outside* the app — an editor, a sync client, a shell script
touching the vault directly. Its round trip through `notify` → `classify`
→ `apply` is unaffected; the app's own writes now simply stop routing
through it, closing the latency window rather than the correctness gap.

`compute::touched` and `compute::removed` are thin: each wraps a single
`VaultChange` in a `Job::Survey` with `escalated: false`, mirroring
`compute::rescan`'s shape. They reuse `refresh`/`absorb`/`survey`
unchanged — a one-note batch runs through the identical incremental
`Index::update_note` / `Index::remove_note` path a watcher batch would,
so there is still exactly one way a change reaches the index.

## Rejected

- **Waiting on the debounce**: the round trip already works, it is just
  slower than the keystrokes that follow a create or a capture. Nothing
  about widening the debounce window or polling harder fixes a latency
  gap that is architectural — the fix is to not depend on notification
  for a write the app itself just performed.
- **Rescanning the whole vault per create**: `compute::rescan` exists and
  works, but it re-reads and re-inserts every note for one new row. A
  personal vault stays small enough that this would not be *slow*, but it
  is the wrong shape for an event the app already knows precisely — one
  path, one category, one change — the same reasoning
  `adr/2026-07-incremental-vault-watching.md` used to justify
  `Index::update_note` over rebuilding on every watcher batch.
- **Reading the watcher's changes and re-emitting them faster**: there is
  nothing to re-emit before the watcher has classified an event notify
  has not yet delivered. The app already knows what it wrote; asking the
  watcher to confirm it is the very round trip being removed.

## Consequence

The stale sentence in `adr/2026-08-watcher-feeds-the-ui.md` — "two
signals are refreshed" — is corrected there: the drain refreshes four
(the rail's time notes, the open loops, the table's notes, and the link
edges), matching `compute::Survey`'s tuple. That ADR now also points here
for the app's own writes.

## Amendment: the category is `from_dir`'s, actually (2026-09-02)

The claim above — each seam carries "the same category the watcher's own
`classify` would have produced" — was true of four seams but not the
fifth: `undo_last` hand-rolled a three-branch derivation (capture,
generated, else Permanent) with no Time branch, so a restored `time/…`
note re-entered the index as Permanent and surfaced as a phantom table
card until a full rescan. That copy now defers to a `ui.rs` helper,
`dir_category`, which reads the path's leading directory through
`NoteCategory::from_dir` — the same authority `classify` itself uses — so
the Time branch it silently lacked is no longer possible to omit. The
2026-09-01 review found this through the loop-line fix, which made an
unreachable time note reachable and so gave the misclassification a door.
