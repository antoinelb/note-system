# The logs' reading column is 720px, the sheet's stays at the printed measure

## Context

`.centre-column` and `.sheet-column` shared one rule, `width: min(529px, 100%)` — the two hosts reconciled onto one fluid reading column by item 8 of `docs/plans/2026-08-31-css-markup-rendering.md`, over the fixed 529px cap `adr/2026-08-one-font-size-for-source-and-render.md` gave `.centre-column`.
529px is 14cm, the page width `templates/template.typ`'s `note()` sets — a measure chosen for paper, inherited by the screen because the editor once drew a compiled SVG of that page and the two had to agree.
CSS draws the markup now (`adr/2026-08-css-draws-the-markup.md`), so nothing on screen is bound to the printed page any more, and the logs' chrome has since shrunk: the rail went from 208px to 176px (`adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md`).
On a 1920px window that left 1398px of pane holding a 529px column — the reading measure was the narrowest thing on a screen with room to spare.

## Decision

**The logs' reading column caps at 720px**; `.centre-column` gets its own rule and `.sheet-column` keeps `min(529px, 100%)`.
The user chose 720 over 800 and over an uncapped column.
Everything else about the column is unchanged: fluid below the cap, `margin: 0 auto` centring it in whatever room the host gives, no horizontal scroll.
Around it sit 177px of rail (176 + its hairline), 281px of jump panel (248 + 32 of padding + its hairline) and `.centre`'s own 64px of padding, so 1242px is the window width below which the logs column starts giving room back.

The two hosts no longer wrap a note at the same measure.
That was never the promise the shared rule was worth keeping for: what `the_logs_and_the_sheet_render_one_note_the_same_way` pins is that one `blocks_view` draws both, each wrapped in its own reading column exactly once — the caps were only ever a coincidence of both inheriting the printed page.

This does not reopen `adr/2026-08-one-font-size-for-source-and-render.md`: that ADR rejected shrinking the *type* to fit a pane, and one `--prose-size` still drives everything.
Widening the column widens the measure, not the letters.

## Alternatives rejected

- **Keeping the printed measure** — it derives the screen's reading width from a paper size nothing on screen renders any more, and it wastes half a 1920px pane.
- **Filling the pane (no cap)** — a 1398px line of 18px prose is far past any comfortable measure, and the column would then change width with every fold of the rail or the jump panel.
- **800px** — the user's call between the two, and the wider cap stops being a cap sooner: the logs' chrome leaves 758px of column at a 1280px window, so an 800px column is already fluid on a common laptop where 720 still holds its measure.
- **Widening the sheet to match** — the sheet is being reshaped separately; its box is the table's own `SHEET_LEFT`/`SHEET_WIDTH` consts, and moving the column inside it is that change's decision, not this one.
