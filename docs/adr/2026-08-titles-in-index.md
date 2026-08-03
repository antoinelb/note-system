# A note's first heading is its title, and the index stores it

## Context

Phase 9 gives `#l` insertion an autocomplete "over note ids/titles from the
index" (`roadmap-v0.md` § Phase 9).
Ids are kebab-case and descriptive — `atomic-notes`, `2026-w30` — and they are
what `#l("…")` renders, so they alone would already be searchable.
But the name a note is *thought* of by is its heading ("How to Take Smart
Notes", "Niklas Luhmann"), and that name was nowhere in the index.

## Decision

- A note's **title is its first level-1 heading**, extracted by `typst-syntax`
  during the same walk that finds `#meta` and `#l`
  (`plan.md` § Known risks: a real parse, never regex).
  Deeper headings (`==`) are sections inside a note, not its name; a heading
  with nothing but whitespace names nothing (`None`).
- The title lives on **`Note`, not `Meta`**: it is content, derived by
  reading the note, not a `#meta` field the user maintains.
  A note may have no title, and that is not debt.
- The `notes` table gains a nullable **`title TEXT`** column, and
  `SCHEMA_VERSION` goes to 2.
  There is no migration: `adr/2026-07-disposable-index-user-version.md` makes
  a version bump a delete-and-rebuild, so a schema change costs one integer.
- `Index::completions()` returns `(id, title)` for every note **with an id** —
  an id-less note cannot be a link target, so it is not a completion (it is
  open-loops debt instead).
  Duplicate ids are not deduplicated: a collision is an error to see, not to
  hide (`adr/2026-07-id-collision-is-an-error.md`).
- The picker matches a query against **either** the id or the title,
  case-insensitively (`links::filter`).

## Rejected

- **Ids only** — smaller, and honest in that the id is what gets written. But
  finding "Niklas Luhmann" then means remembering the note was filed as
  `luhmann`, which is exactly the recall the picker exists to remove.
- **A `title` field in `#meta`** — a second place to keep the same string in
  sync, and every template would have to seed it. The heading is already
  there and already the title.
- **Regex over the source** — the parse tree is already being walked; a
  regex would trip on `=` inside raw blocks and equations.
- **A separate `titles` table** — one row per note, joined on path, for a
  column that is one-to-one with the note. A column is the same data with
  less SQL.
