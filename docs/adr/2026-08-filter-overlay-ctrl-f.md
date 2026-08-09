# Ctrl+F filters by tag or type; filtered-out cards dim

## Context

Phase 7 needs filters by tag and by type, with filtered-out cards dimmed, never removed — spatial memory is the point, and holes would break the map.
The summoning keystroke and the overlay's shape were undrawn in the deck.

## Decision

- **Ctrl+F**, table-only, summons the link-picker-pattern overlay: one query input over one list holding every tag then the eight permanent type names; typing narrows by the contains rule; Enter applies the highlighted entry and closes; Escape closes without changing anything.
- **One filter at a time** — applying replaces the previous one; tag and type filters don't compose in v1 (the ceiling).
- **Enter on an empty query clears the active filter** — the re-summon-and-clear gesture; there is no clear button because there are no buttons.
- **Filtered-out cards dim** (`.card.dimmed`, opacity — not a colour, no palette change), and a type filter dims captures, generated and untyped notes too: they are not the type asked for.
  Matching keys on the note's own `#meta` (`note_type`, `tags`), not the presented kind.
- **The active filter shows in the chrome** as a small label ("tag · method" / "type · concept") — the map reads differently under a filter, and the chrome must say why.
- The overlay reuses the palette's floating box and the picker's row grammar wholesale; it renders inside the table branch and the screen switch clears it — an overlay waiting behind a screen would reopen unasked.

## Rejected

- **Multi-select filters (OR of several tags)** — more power, more state to display and test; the v1 list is the ceiling.
- **Hiding filtered-out cards** — explicitly against the roadmap: dim, not disappear.
- **Filtering the index query** — the dim set is presentation over the same cards; a narrower query would unmount cards and lose their positions' visual anchor.
