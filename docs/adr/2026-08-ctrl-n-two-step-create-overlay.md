# Ctrl+N creates a permanent note through one two-step overlay

> Amended by `2026-09-a-course-is-a-project.md` and `2026-09-tool-is-the-ninth-permanent-type.md`: step 1 lists nine types now — the eight below, `course` never among them again, plus `tool` last.

## Context

Phase 4 needs creation without leaving the table: a keystroke, a type, a title — `template::create` needs the last two, and the id derives from the title.
The design has no buttons, so the gesture is an overlay in the link-picker grammar (`adr/2026-08-command-palette-overlay-shape.md`).

## Decision

- **Ctrl+N** summons the overlay on both screens; "new note" joins the palette with that chord.
- **One overlay, two steps.**
  Step 1: the eight permanent types (person, organisation, source, concept, claim, idea, personal, project), typing filters by the palette's contains rule, Enter picks the highlighted type.
  Step 2: the same input empties and becomes the title prompt (the head shows the picked type), Enter creates from the type's template and opens the new card's sheet.
- **Escape backs out step by step**: title → type list → closed, with the palette's focus-restore machinery on the final close.
- **Creation errors stay in the overlay.**
  `AlreadyExists` and `EmptyId` render as a notice line inside the overlay, which stays open so the title can be amended — on a bare table no editor notice line is visible, so the message must live where the user is typing.
- The new sheet opens through the created path directly, never through the index lookup — the watcher's debounce means the index learns the note ~200 ms later.

## Rejected

- **A single "type: title" input** — a parse rule to memorize and no visible list of types; the two-step keeps the vocabulary discoverable.
- **Eight palette commands ("new concept", …)** — two overlays deep for every creation, and the palette registry would grow eight near-duplicates.
- **Surfacing errors on the editor notice line** — invisible when no sheet is open.
