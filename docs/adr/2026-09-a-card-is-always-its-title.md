# A card is always its title: the body level leaves the table

## Context

The table drew a card two ways, read off the scale: a title under 2.0 and the note's whole rendered SVG in a clipped 296-tall box at 2.0 and above (`adr/2026-08-body-zoom-scale-and-metrics.md`, `adr/2026-09-the-table-zooms-continuously.md`).
The body level carried its own machinery: a per-note SVG cache invalidated by the watcher and by every write seam (`adr/2026-08-body-cache-per-note-svg.md`), a `Body` job and outcome on the compute tier, a stale shelf, an epoch, a pending gap, a second card height for culling, and a palette pair to jump between the two stops.
In use the level was a curiosity, not a reading surface: a note is read on the sheet, and a constellation of 296-tall cards is a wall of unreadable thumbnails whose footprints the resolver had to ignore anyway (`adr/2026-09-cards-yield-on-drop.md` resolves against the 56-tall title box at every scale).

## Decision

- **A card draws its title at every scale.** `table::Zoom`, `BODIES_AT`, `BODIES_SCALE`, `BODY_HEIGHT` and `BODY_CARD_HEIGHT` are gone; `in_view` culls against the one title height.
- **The body pipeline is deleted whole**: `render::BodyCache`, `BodyView`, `BodyJob`, `compute::Job::Body`, `Outcome::Body`, the `.card-body`, `.card.bodies` and `.body-pending` rules, and every `bodies.invalidate` call on the write seams.
  Export still compiles whole notes; the fragment cache still draws what CSS cannot.
- **The zoom itself is unchanged**: still one `f64` from 0.25 to 4.0, stepped by ×1.1 around the pointer or the pane's centre (`adr/2026-09-the-table-zooms-continuously.md`).
- **"zoom to titles" is the one jump**, to exactly 1.0, and it hides while the table already stands there (`palette::Context::zoomed`, replacing `at_bodies`).
  "zoom to bodies" is gone; a `usage` file still carrying its count rides along unread, as unknown keys always have.
- `Card` loses its `path`: the body compile was its only reader.

## Rejected

- **Keeping the body level behind a setting.** Configurability for a level nobody reads, plus the whole cache kept alive for it.
- **Drawing the body with CSS from the markup model instead of an SVG.** The same wall of thumbnails, drawn cheaper; the objection is to the level, not to its renderer.
- **A third, farther level (dots) in its place.** The continuous zoom at 0.25 already is the constellation view.
