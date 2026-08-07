# The open note carries a both-directions link footer

## Context

Phase 9 asks for a "backlinks panel on the open note" and "dangling links
visibly marked (in the panel at minimum)".
The deck never drew a v0 backlinks surface: the vocabulary available is the
v1 writing sheet's footer ("← 2", `design/wireframes-v0.md` § Chrome) and
turn 3's rejected shell, whose "backlinks/outgoing footer with dangling
marked" the deck explicitly kept as *editor* vocabulary.

## Decision

- One **footer under the rendered note** in the logs centre pane, above the
  "captured today" block — the footer belongs to the note, that block belongs
  to the day. Two rows: **`←`** for the notes linking here, **`→`** for the
  ones this note links to. A direction with nothing in it renders no row, and
  a note with neither renders no footer: absence, not an empty frame, the
  same grammar as the open-loops ember.
- **Outgoing links are read from the live buffer**, not the index. The
  watcher is not wired into the UI yet (the index is built once at launch), so
  an index-backed outgoing query would go stale the moment you type. Parsing
  the buffer with the existing `typst-syntax` parser means a link marks
  itself dangling *as it is typed*, and stops the moment its note exists.
- **Backlinks come from the index**, which is right for them: they depend on
  *other* files, and those cannot change in-session until the watcher lands
  in phase 10. (It has, `adr/2026-08-watcher-feeds-the-ui.md`: the index now
  keeps up with the vault, and both halves of this decision still hold — the
  buffer is still fresher than any index for the note being typed into.)
- **Clickable only where the app can go.** A time note opens in the centre
  pane, so its entry jumps; a permanent, capture or generated note has
  nowhere to be shown until v1's table
  (`adr/2026-07-permanent-notes-wait-for-table.md`), so its entry is visible
  but inert. A backlink from a note with no id is labelled by its filename —
  it is debt of its own, but still a real link.
- **Dangling is `--ember`**, the palette's one warm hue, which
  `assets/theme.css` already declares as the alert hue. Nothing else marks
  it: no icon, no "(missing)", no per-item action.
- **Self-links are filtered out** of both directions. A note linking to
  itself is a real row in the `links` table — debt the open-loops list can
  have — but it tells the reader nothing.
- Typography follows the **"captured today" block** (16px Lato, muted ink,
  a hairline above), not the mockups' 9–10px mono: the reading scale was
  bumped a step in `adr/2026-07-reading-scale-bumped.md`, and the footer is
  read at the same distance as the note.
- An index that cannot answer becomes a visible error line, never a note
  quietly full of ghosts — target existence is resolved before classifying,
  so a failed query cannot masquerade as "this link is dangling".

## Rejected

- **Backlinks only, like the v1 sheet's "← 2"** — smaller, and true to the
  drawn design, but phase 9 also wants dangling links visibly marked and the
  ember is only a count. Both directions in one place answers both asks.
- **A count instead of the ids ("← 2")** — that is the *table's* card
  vocabulary, where space is scarce. In the centre pane the ids fit, and the
  ids are what you would click.
- **A section in the jump panel** — keeps the centre pane pure prose, but the
  jump panel is the time axis; the note graph does not belong in it.
- **An index-backed outgoing query** — symmetric with backlinks, one fewer
  parse per render, but wrong until the watcher is live, and it would make
  the freshest thing on screen the stalest.
