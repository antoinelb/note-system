# A typed capture wears its type's treatment

## Context

Capture promotion is editing, not a feature: set `type` in `#meta`, write the summary — the file stays in `capture/`, nothing moves it.
But the card treatment keyed on the directory-derived category: `bar_class` and `label` sent every capture to the grey age-label treatment, so a promoted capture repainted nothing — the roadmap's "the card regains a hue and full fill" was unreachable.

## Decision

- **Presentation keys on the type**: in `table::cards`, a capture whose `note_type` is one of the eight permanent types builds its card with `kind: Permanent` — type label, type bar, full fill all follow from the one switch.
  `Card.kind` is presentation; the index category and the file's directory stay honest.
- A capture with no type, or an unknown one, keeps the capture treatment — the age label is friction and stays until a real type lands.
- No affordance beyond the editor: promotion is typing `type: "concept"` and a summary in the sheet; the watcher re-indexes and the card recolours.
  The round-trip is test-enforced (`promotion_recolours_through_the_watcher`).

## Rejected

- **Moving the file to `permanent/` on promotion** — the app rewriting files the user is editing crosses the "plain files are the source of truth" line for a cosmetic gain; category-by-directory stays untouched.
- **A "promote" command** — a second way to do what typing already does, and the affordance suggests promotion is more than editing.
- **A separate `fill` field on `Card`** — switching `kind` at build time is one change; a parallel field would let the treatments drift apart.
