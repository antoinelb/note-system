# Time navigation is derived, never seeded into the file

## Context

`daily.typ` currently carries `#l("{{prev}}") | #l("{{next}}")`, and the roadmap's phase-5 test line asks for "daily prev/next resolution across gaps".
Filling those two slots at creation time turns out to be impossible in one direction and harmful in the other.

**`{{next}}` cannot ever be valid.**
When today's note is created, tomorrow's file does not exist.
Any value substituted into `{{next}}` is a link to a file that is not there.

**A seeded dangling link is not free.**
Phase 3 made dangling links visible debt (`index.rs` `dangling_links`), and the open-loops panel reports them.
Seeding one forward link per daily note produces 365 permanent false loops a year — the friction machinery firing on something that is not a problem.
That directly undercuts the point of the friction system: debt is only useful if every entry is real.

**The phase-1 fixtures already voted.**
`time/2026-07-21.typ` links forward only, `time/2026-07-23.typ` links backward only, and only the middle note has both — each note links to files that exist.
Those links were hand-placed, which is exactly what template substitution cannot reproduce.

**`plan.md` contradicted itself in one sentence** (§ Note model): "prev/next navigation links … the day/week/season chain is derived from the id convention, not from stored links."

## Decision

**The templates seed no navigation links.**
`{{prev}}` and `{{next}}` are removed from `daily.typ`, and `weekly.typ`/`seasonal.typ` (slice 4) will not have them either.

Movement between time notes is **computed from the id convention plus an index existence check**, never read from the file:

- *scale chain* (day → its ISO week → its season) — rendered as app chrome above the centre pane on the logs screen (`design/wireframes-v0.md` § The logs screen), by `jiff` date math;
- *previous / next day* — a navigation action that resolves to the **nearest existing daily note** in that direction, which is what "resolution across gaps" means and is testable; a stored link cannot cross a gap because it was written before the gap existed.

Until the logs screen lands in phase 7, navigation is the phase-4 note list.

Consequences:

- **A user writing `#l("2026-07-22")` by hand is still normal and correct.**
  This decision is about what the *template* seeds, not about what a note may contain — the existing fixtures keep their hand-written links, and the integration test that asserts on their order stays valid.
- **The design agrees.** The writing sheet keeps "exactly two lines of its own chrome: the in-note meta line at the top, and a footer showing only backlinks" — there is no prev/next chrome to build, and no room to invent one.
- `plan.md` § Note model loses "prev/next navigation links" from the Obsidian-pattern list; the derived-chain clause it already contained is now the whole story.

## Alternatives rejected

- **Fill `{{prev}}` only, drop `{{next}}`** — backward links are always valid-or-absent, so this is defensible.
  Rejected because it buys a link the app can compute anyway, at the cost of an index lookup during creation and a template that renders differently on the first daily note ever written (no previous day: an empty line, or a dangling `|`).
  A stored `prev` also goes stale in the one case that matters — write a note *between* two existing days and the older note's forward link still skips over it.
- **Keep both and accept the dangling `next`** — closest to the current Obsidian habit, and to `plan.md`'s literal wording.
  Rejected on the 365-false-loops-a-year cost above.
- **Have tomorrow's creation back-fill yesterday's `next` link** — makes both directions real, but it means the app edits a note the user is not looking at, to insert prose-level content it decided on.
  Even though a link is not prose, silently rewriting a closed note is the kind of thing this project's AI invariant exists to prevent; the same instinct applies to non-AI code.
