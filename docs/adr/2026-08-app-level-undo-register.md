# The undo register: delete and arrange gain a reverse

## Context

Exactly one history existed — the editor's vim-grain one, scoped to the
open buffer. Deleting a note (irreversible) had less mechanism than
pressing `x` on a character, even though the delete callback held the full
text at the moment of destruction and threw it away; an `arrange` moved a
whole cluster with no way back.

This reopens the ground the delete decisions stood on
(adr/2026-07-delete-unconfirmed-no-trash.md,
adr/2026-08-delete-note-palette-only-from-sheet.md) without reversing
them: those ADRs rejected *confirmation and trash* — dialogs are theatre,
a trash is a second vault — and never evaluated *undo*. The 2026-07 ADR's
own revisit point ("when the logs screen replaces the scaffolding shell")
has passed. Delete stays unconfirmed and trashless; it just leaves a
before-image behind.

## Decision

Taken with the user (2026-08-13):

- **An `undo` module holds a bounded in-memory register** (depth 10) of
  before-images, captured at the moment of destruction: a delete keeps the
  note's text and its card's coordinates, an arrange keeps every moved
  card's prior position (`None` for auto-placed — their reverse is
  unpinning). The register dies with the app; git stays the deep recovery.
- **Two intents: delete and arrange.** Drags stay un-undoable — they are
  continuous and self-correcting by dragging back.
- **One palette command, "undo", hidden while the register is empty.** The
  rendered row wears the register's words for what it would take back —
  "undo delete <id>", "undo arrange". Chordless like delete: the reverse
  of a summon-and-name is a summon-and-name.
- **An undone delete returns by `create_new`, never a clobber**: if the
  path holds a living file again, the undo refuses with a warning notice
  and the recreated file stands. The card itself reappears when the
  watcher converges — the same trust the delete's own landing half leans
  on.

## Rejected

- **Confirmation or trash** — already rejected by the delete ADRs, and
  undo answers the actual risk (a mistaken delete) without taxing every
  intended one.
- **Per-kind commands** ("restore deleted note", "undo arrange") — two
  entries teaching two vocabularies for one idea.
- **Persisting the register across launches** — a shadow vault by another
  name; git already is the durable history.
- **Drag undo** — the gesture corrects itself; the register is for
  one-shot destructions.

## Reaffirmed by `2026-09-shift-drag-selects-cards.md` (2026-09-06)

`docs/plans/2026-09-multi-select-cards.md` argued that "the gesture
corrects itself by dragging back" had been voided by the yield rule
(`2026-09-cards-yield-on-drop.md`): dragging a group back does not un-yield
the neighbours it pushed. The user reaffirmed the rejection anyway — no
`Intent::Move`, single or group. A drag's consequence on its neighbours is
accepted as the price of the layout rule, and the register stays what it
is: a bounded list of one-shot destructions.
