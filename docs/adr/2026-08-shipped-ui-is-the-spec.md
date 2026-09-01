# The shipped UI is the spec; the wireframe doc's intent survives here

## Context

Commit `ed91492` deleted every prose document under `docs/` except the ADR
register: `plan.md`, the four roadmaps, `interface-rules.md`, and
`design/wireframes-v0.md` with its mockup deck. Almost everything they said
is carried elsewhere — the palette in `assets/theme.css`, the roadmap and
invariants in `CLAUDE.md`, behaviour in the code and its ADRs. But
`wireframes-v0.md` Part I was the operative UI spec, and its *intent* — the
principles the pixels follow — existed nowhere else. Without a record, the
next redesign re-litigates six turns of iteration.

## Decision

- **The shipped screens are now the authority.** `theme.css` carries every
  colour, the code every layout and state; the old rule "on conflict the
  wireframes win" (`adr/2026-07-plan-realigned-with-wireframes.md`) is
  retired — there is nothing left for the code to conflict with.
- **The design's identity is preserved as intent, not pixels.** The look is
  *Deep field* in *one-line* chrome: indigo void, link edges as
  constellations with star nodes, the amber ember/caret as the only warm
  element, and the whole app's chrome being one top line (two stroked icons
  plus the loop count) and two lines on the writing sheet (meta line,
  backlinks footer). Lineage, for the record: card-table metaphor × chromeless
  writing surface (turn 1), free positions with a tethered sheet (2b), layout
  3a table · 3e logs, mood 4c, chrome 5b, states frozen in turn 6.
- **The named principles bind future UI work:**
  - *Absence is the reward* — zero open loops means no ember, not a "0".
  - *Selection ≠ existence* — a selected empty calendar day is outlined,
    an existing one filled.
  - *Empty is honest* — no ghost template; only Enter creates a file,
    navigation never does.
  - *Proposals never interrupt* — one dashed line at the end of the page;
    accepting means the user writes.
  - *The tether keeps place legible* — the open card's edge to the sheet is
    the one thing left undimmed.
  - *The app never restyles note bodies* — close zoom is rendered typst,
    the template's own output.
    *(Amended by `adr/2026-08-css-draws-the-markup.md`: narrowed to what
    ships as output — `typst compile`'s export path and the table's card
    bodies, which still never touch a CSS renderer. The editor's own live
    view now draws most blocks as CSS spans read off the same parse tree
    the compiler reads, which restyles note bodies in the sense this
    principle names; the amendment ADR argues why that carve-out holds.)*
  - *Words, not buttons* — no borders, chips, or keyboard hints anywhere.
- **Still open from the deck**, never drawn or built: the open-loops screen
  in this visual language, and the table at titles zoom with a dense
  (~30-card) population. The `seasonDisplay` knob (whether the season scale
  shows at all) also remains a live tweak.
- **Full text is recoverable** at
  `git show ed91492^:docs/design/wireframes-v0.md` (the deck sits beside
  it). Live-file citations of the deleted docs retarget to this ADR.
- **ADRs cite the deleted docs two ways, and only one of them is dead**
  (amended 2026-08-24, after the first sweep left ~110 broken pointers
  behind). A *pointer* — a parenthetical propping up a claim, `(plan.md
  § Editor)` — retargets to the live ADR that carries the claim, or is
  dropped and the prose kept. A citation that is the *subject* — recording
  what a deleted doc said, or an edit made to one — stays verbatim: that is
  the record, and rewriting it would falsify history. Roadmap phase names
  survive as prose either way; only the file pointer dies.

## Rejected

- **Keeping `wireframes-v0.md` as the one surviving prose doc** — it
  described mockups the implementation already outgrew (spacing normalized
  to multiples of 4, English strings, knobs picked); a spec that loses to
  the code on every conflict is documentation debt, not documentation.
- **Letting git history carry it alone** — the principles and the unbuilt
  states would be invisible to anyone reading the register, which is the
  point of having one.
- **Reproducing the palette and layout tables here** — `theme.css` and the
  code carry every value; a copy is a second source of truth waiting to
  drift.
