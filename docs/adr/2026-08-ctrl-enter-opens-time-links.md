# Ctrl+Enter follows the link under the caret

## Context

Phase 9 makes links cheap to write (Ctrl+L) and visible in both directions
(the footer), but leaves them one-way in practice: the only way to *follow*
a link is to reach for the mouse and click a footer entry.
The keyboard already owns the rest of the writing loop, so the last step of
"links are cheap" is following one without leaving the caret.

## Decision

- **Ctrl+Enter, over an active block, opens the `#l("...")` the caret stands
  in.** The chord already bubbles out of the textarea to the logs pane, where
  Ctrl+L lives, so it needs no new plumbing — it needs the editor, the caret
  probe and the note list, none of which the app root has.
- **Ctrl+click in the source does the same thing**, through the same code
  path: the click has already moved the caret, so the same probe answers
  where it landed and one `follow_link` callback serves both. It applies to
  the **active block only** — a rendered block is a compiled SVG with no
  mapping back to source offsets, so there is nothing to ask about a click
  in it. A plain click is unchanged: it just places the caret.
- **The modifier is matched in the pattern**, `Key::Enter if
  event.modifiers().ctrl()`, so the arm always wins ahead of the plain-Enter
  create-the-selected-note arm below it. A chord that finds no link returns
  inside the arm rather than falling through: Ctrl+Enter must never write a
  file.
- **Time notes only.** A permanent, capture or generated target has nowhere
  to be shown until v1's table
  (`adr/2026-07-permanent-notes-wait-for-table.md`), so the chord is inert
  over it — exactly the rule the footer's clickability already follows, and
  the same `links::scale_of` decides both.
- **The link is found by parsing, never by scanning text.** `links::link_at`
  reparses the active block with `typst-syntax` and walks it tracking byte
  offsets, pruning every subtree whose span misses the caret, so the walk
  descends one root-to-link path. This keeps the "a real parse, never regex"
  invariant, and a link with no string target leads nowhere — the same
  reading the footer gets from `parse_note`.
- **The caret probe answers in UTF-16 units within the active block**, the
  same anchor Ctrl+L and the boundary arrows read, converted through the
  existing `blocks::byte_offset_of_utf16`.
- **Both edges of the call count as inside, and so does the `#`.** typst
  tokenizes the hash as the call's left sibling rather than part of it, so
  `link_at` probes one byte on when the first probe misses; otherwise the
  chord would be dead exactly where Home leaves the caret on a line that
  opens with a link.
- **Navigation goes through the existing `select` callback**, so following a
  link behaves precisely like clicking the rail, a crumb or a footer entry —
  including leaving the pending edit to the autosave rather than flushing.

## Rejected

- **Following the link on plain Enter, or on a click in the source** — Enter
  is the newline the block editor needs, and a click in the source is how the
  caret is placed. The chord has to be one the caret does not already own.
- **A text scan for `#l("` around the caret** — shorter, but the project
  parses typst with `typst-syntax` everywhere precisely so that a link inside
  a comment, a string, or nested markup is judged by the grammar rather than
  by punctuation.
- **Opening permanent notes too** — the centre pane can render any note
  (`Editor::open` takes any path), so the temptation is real, but the id
  would need an index round-trip and, more importantly, the rail, the crumbs
  and the empty-day create keystroke all assume the selection is a time note.
  That is v1's table, not a side effect of a chord.
- **Flushing the buffer before navigating** — consistent with nothing else on
  the screen; the autosave already owns that timing.
