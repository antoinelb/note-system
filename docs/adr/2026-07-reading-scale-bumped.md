# The reading scale bumps one step and the editor wears the prose face

## Context

First real writing sessions in the hybrid editor found the frozen type
scale small at desktop distance, the mono source visually jarring inside a
Parisienne page, and the 328px jump panel wider than a month grid needs.
The wireframes' values were tuned on mockups, not in the running app; on
conflict the running hand wins.

## Decision

- Prose bumps one step: body 15px → 18px (13.5pt), title 26px → 32px
  (24pt); the meta line stays 9px. `template.typ` § note().
- The active block's textarea drops the mono face for **18px Parisienne**,
  matching the rendered body — entering a block changes what is editable,
  not the costume. The raw typst markup stays visible; only the letterforms
  match the page.
- The jump panel narrows to 248px (grid font 16px → 14px, gutter 28px →
  24px): it is a jump panel, not a screen, and the centre column gets the
  reclaimed width (which also scales the rendered fragments up).

## Rejected

- **Scaling the SVGs via CSS instead of the template** — zooms the paper,
  not the type scale, and the source pane would still disagree with it.
- **Keeping mono for source** — the classic choice, but the editor's unit
  is a prose block, not code; alignment never matters and the mode switch
  reads better when only the markup glyphs change.
