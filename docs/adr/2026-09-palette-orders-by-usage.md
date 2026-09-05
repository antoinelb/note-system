# The palette orders by usage, counted in a plain-lines file beside the positions

## Context

`2026-08-palette-order-and-overlay-placement.md` sorted the registry alphabetically and made the array itself the sorted order, explicitly rejecting a sort at query time: "reordering the constant once is free and keeps `filter`'s 'walks the array in place' contract intact."
Alphabetical is a good order for a stranger and a poor one for the person who has used the app for a year: the four rows reached daily sit wherever the alphabet put them, and the twenty-nine reached twice a year sit in front of them.
The user asked for the palette to be ordered by what is most often used.

Two questions had to be answered before code: where the counts live, and what happens to ties.

## Decision

**A plain-lines file at `vault/.index/usage`, the positions file's sibling.**
A count of what the user reached for is user data with no upstream — nothing in the notes derives it, and a rebuilt index cannot reconstruct it.
That is the same property `2026-07-positions-separate-file.md` used to keep positions out of the database, and the same reason applies unchanged: the very disposability that makes `DROP TABLE` safe for the index makes it unsafe here.
`src/usage.rs` is `src/positions.rs`'s shape line for line:

- One entry per line, `command count`, whitespace-separated, no extension on the filename (`2026-08-positions-plain-lines-file.md`'s format decision, reused rather than re-argued).
- Loaded once at mount; a missing or unreadable file is "nothing counted yet", never an error, and a malformed line loses that line, not the file.
- Unknown keys ride along and survive a save, so a key from an older build or a hand-edit is preserved rather than silently dropped.
- Every save is write-temp-then-rename through `persist::write_atomic` (`2026-08-atomic-persist-seam.md`); the parent is ensured first, because a command can be run before the first survey has created `.index/`.
- A refused write is reported on the status surface as its own `Source::Usage`, a warning rather than a critical: the command itself ran and no prose is at stake, only the ordering's memory. A later save that lands resolves it, the same gate every other source uses.

**The key is the `CommandId`, not the label.** `usage::key` is an exhaustive match from the enum to a frozen kebab-case name (`toggle-theme`, `open-daily`), so a row renamed in the registry keeps its history, and a variant added to the registry does not compile until it is named.

**Counts descending, ties in the registry's own alphabetical order.** `palette::filter` collects the matches exactly as before — the query's matching rule is untouched, and availability still hides what the context hides — then applies one stable `sort_by_key` on `Reverse(count)`. Because the registry array is already alphabetical and the sort is stable, every count-zero row keeps its alphabetical place: **a fresh install's palette is byte-for-byte the palette of the day before this change**, and the order only diverges as the user's own history accumulates. This reverses that one rejected alternative of `2026-08-palette-order-and-overlay-placement.md`: the sort now happens at query time, because the order is no longer a property of the array.

**A run is a run from the palette.** `run_command` — the one path Enter and a row click share — records and saves before dispatching, so `quit` is counted even though the command it runs closes the app. A chord that bypasses the palette (Ctrl+D, Ctrl+N, Ctrl+Shift+V) counts nothing: the palette orders the list it shows by how the list itself was used, and a chord is by definition a gesture that needed no list.

The reorder is never seen moving. It is computed when the palette opens and while the query changes, and a run closes the overlay before the count changes anything — nothing already on screen moves as a result of the write (AIR LAY-1).

## Alternatives rejected

- **A `usage` table in the index database** — one fewer file, but the index is deliberately disposable (`2026-07-disposable-index-user-version.md`), and the exception would have to be remembered by every future rebuild. Exactly the failure the positions ADR made structural instead of remembered.
- **Session-only counts, like the theme and the font size** (`2026-08-settings-overlay.md`) — those are two knobs with a defensible default on every launch; a usage history that resets each launch is worse than useless, since the order would churn within a session and reset before it could pay off. "No config file" was a stance about a *settings* file for two numbers, not about user data.
- **Recency instead of frequency (an MRU list, the switcher's rule)** — Ctrl+O lists the visit log newest first because a note switcher answers "what was I just in". A command palette answers "what do I always reach for", and the command run once yesterday should not outrank the one run every morning. Frequency also needs no timestamps in the file.
- **A decayed or windowed count (last 30 days, exponential decay)** — needs a clock in the file format and a policy nobody asked for; a plain integer is debuggable at 3 AM and a wrong order costs one extra typed character.
- **Counting chord invocations too** — a chord's speed is its point, and a command already fast by chord does not need to also be first in a list it is never opened through. Counting them would let the daily chords permanently crowd the top of a list they are never read from.
- **Sorting the array at startup instead of at query time** — the counts change under the palette while the app runs; a query-time stable sort over thirty-three entries is free and always current.
