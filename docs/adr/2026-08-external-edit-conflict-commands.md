# The external-edit guard and the conflict's palette fork

## Context

With plain `fs::write`, editing the open note outside the app (vim, a sync tool) was silently overwritten by the next autosave ~500 ms after the next keystroke — last-writer-wins data loss with no notice.
The AIR review (C2's extension) flagged it; the app has no merge UI and never will at this scale.

## Decision

Taken with the user (2026-08-13):

- **The buffer keeps a stamp** — the mtime of the version it last read or wrote (`write_atomic` returns it race-free). Before every save the guard compares the disk against the stamp; a mismatch refuses the save as a **conflict**: nothing is written, both versions survive.
- The stamp is taken **before** the open's read, so an edit slipping between stat and read surfaces as a refusal instead of being silently missed.
- A **vanished file is not a conflict** — recreating the user's own text over a hole erases no one; an unresolvable stamp leaves the guard off rather than blocking every save.
- **A conflict is its own status source at critical severity**, not a save failure: an io failure resolves when a later save lands, a conflict only when the user picks a side.
- **Resolution is two chordless palette commands**, existing only while a conflict stands (hidden beats disabled): `keep mine` clobbers the disk with the buffer and re-arms the guard; `take disk` reloads the buffer from the file, discarding in-app edits. Picking a side resolves the notice; a keep-mine the disk refuses resolves nothing.
- **Quit and sheet navigation stay blocked** while the conflict stands — the existing failed-flush semantics, and honest: the app genuinely cannot decide for the user.

## Rejected

- **Warn once, then overwrite** — the protection window is one debounce tick; an unattended app still clobbers.
- **Escape-acknowledge arms the overwrite** — dismissing a message would silently become destroying the disk version; reading is not deciding.
- **Refuse with no resolution gesture** — a permanently refused flush would make quit impossible.
- **Reusing `Source::Save` for the conflict** — the autosave's own success path resolves that source, which would clear a conflict no one decided.
- **Content hashes instead of mtime** — a full disk read per tick to close a sub-granularity race the epoch-backdating tests show mtime already handles; the stamp is cheaper and the guard is best-effort by design.
