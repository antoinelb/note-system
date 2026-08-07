# A capture is summarized when its Summary section is not empty

## Context

The friction system's first kind of debt is "captures without a self-written
summary" (`plan.md` § Friction system), and phase 10 has to say what that
means in code before the ember can count it.

The capture template already answers it structurally: it writes
`== Summary` then `== Original`, and the pasted content lands under
*Original*. Promotion is "choosing a type + writing your own summary", so
what distinguishes a closed loop from an open one is whether the user typed
anything under that first heading.

## Decision

- **Summarized = any non-whitespace content between the `== Summary`
  heading and the next heading of depth 1 or 2.** Nothing else is consulted:
  not the note's type, not its length, not a marker field.
- **Detected at parse time**, in `parse_note`, as one linear pass over the
  root's top-level children — typst makes sections siblings separated by
  `Parbreak`, the same structural fact `blocks::segment` relies on. The
  fine-grained walk `parse_note` uses for `#meta` and `#l` would have been
  wrong here: it pops a heading before its body, so the word "Summary"
  itself would read as content.
- **A deeper heading inside the section is content**, not the section's end.
  Someone who structures their summary has written a summary.
- **Stored as a `notes.summarized` column** (schema 3). The index is
  disposable, so this is a version bump and a rebuild, never a migration
  (`adr/2026-07-disposable-index-user-version.md`). Every note gets the
  column; only captures are ever asked about it, since a permanent note owes
  a summary to nobody.
- **The heading text is matched case-insensitively** against the template's
  own `Summary`. Case is the one variation a hand-written capture plausibly
  has, and reading it strictly would leave a filled summary counting as debt
  with no way for the user to see why.
- **The heading name is a contract between `templates/capture.typ` and the
  parser.** Translating the template — the notes' own language is the
  user's — means changing `SUMMARY_HEADING` with it. This is the one place
  the app reads a note's prose structure rather than its `#meta`.
- Where an unsummarized capture is listed, it is tagged **"still open"**
  rather than by its category: in the day's "captured today" block and in
  the open-loops list alike, one string, one meaning.

## Rejected

- **Promotion (gaining a `type:`) closes the loop** — coarser and later:
  writing the summary would not clear the debt, which contradicts what the
  plan calls the loop ("a capture *without a self-written summary*"). Type
  and summary are two halves of promotion, and the debt tracks the half the
  user must write themselves.
- **An explicit marker (a tag, a `#meta` field)** — a convention the
  template does not carry, requiring the user to declare what the prose
  already shows, and one more thing to forget.
- **Length or word-count thresholds** — arbitrary, and they would make the
  app an editor of the user's summary rather than a reader of it.
- **Detecting it in `blocks::segment` instead** — the block tiling could
  answer it too, but every other fact the index knows about a note comes out
  of `parse_note`, and a second structural reader would be a second thing to
  keep true.
