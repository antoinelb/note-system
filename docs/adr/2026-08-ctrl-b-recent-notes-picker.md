# Ctrl+B opens a recent-notes picker instead of jumping straight back

## Context

`adr/2026-08-note-history-back.md` gave Ctrl+B Obsidian's Ctrl+O shape as a blind pop: one press, one jump to the top of the visit stack, with a `restoring_history` flag suppressing the landing's own push so repeated presses walked backward instead of oscillating.
In use the blind jump was judged wrong (user, 2026-08-26): you cannot see where you are about to land, and reaching anything but the newest visit means jumping through every note between.
The final review had also flagged that two callers (`follow_at`'s time-link branch, the table's Ctrl+D) closed the sheet before `select` pushed, so the log recorded the logs selection under the sheet instead of the sheet itself.

## Decision

- **Ctrl+B opens a picker** in the jump overlay's grammar (`adr/2026-08-jump-ctrl-o-centres-viewport.md`): a `command-palette` box headed "recent notes", a query input, arrows, Enter, Escape, clickable rows.
- **The rows are the visit log's distinct notes, newest first, minus the place currently showing**, frozen at open — the `Picker` idiom.
An empty list is a silent no-op, unchanged from the pop version: there is nowhere behind the first note.
- **Landing is a real visit.**
No pop, no push suppression: the note you left becomes the log's newest entry, so the picker can bounce, and duplicate log entries are folded at display time instead of being prevented at push time.
`restoring_history` survives with exactly one user, the template-editing Escape, whose return through `select` is not a visit; dedup-plus-exclusion also makes any residual self-visit invisible in the picker.
- **Callers that leave a sheet run `select` first and do their screen hygiene after**, because `select` reads the sheet for its push before closing it — `follow_at` no longer calls `close_sheet` ahead of `select`, and the table's Ctrl+D runs `open_daily` before `go_logs`.
This fixes the final review's stale-push finding and is what makes the picker's rows truthful.
- The palette row is relabelled **"recent notes"** and re-sorted to its alphabetical place; it opens the picker like the chord does.

Supersedes the gesture half of `adr/2026-08-note-history-back.md`; the visit log itself — what pushes, the 64-entry cap, `Visit::Logs`/`Visit::Sheet` — is unchanged.

## Rejected

- **Keeping the blind pop** — the redesign's trigger; a destination you cannot see is the wrong grammar for a log deeper than one entry.
- **A pop-walk plus a picker on long-press or double-press** — two grammars for one key, and the pop half keeps the oscillation machinery (`restoring_history` on every landing) the picker made unnecessary.
- **Mutating the log on pick (jumplist-style splicing)** — vim's jumplist semantics are subtle and invisible; a log that only ever appends and caps is predictable, and display-time dedup gives the picker the same tidiness.
