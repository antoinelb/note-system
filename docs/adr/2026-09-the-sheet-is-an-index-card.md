# The writing sheet is an index card

## Context

The sheet opened as a tall panel pinned to the left of the table: `SHEET_LEFT: 440`, `SHEET_WIDTH: 620`, 44px from the top and bottom of the pane, frozen at the deck's knobs by `adr/2026-08-sheet-knobs-frozen-at-deck-defaults.md` before daily-driving could say anything about them.
Daily-driving has now said it.
A full-height column two thirds of the way across a wide window reads as a web page docked to a side, not as something taken off the table and put down in front of you — and it is off-centre, so the eye leaves the middle of the screen to write.
The cards on the canvas are index cards; the thing a card opens into was the one surface that did not look like one.

## Decision

**The sheet is an index card: a landscape 5:3 rectangle, centred on the table, at most 900 x 540, and never nearer than 44px to a pane edge on either axis.**
`table::sheet_frame(viewport)` computes the whole frame — left, top, width, height — from the pane size the table already observes (`onresize`, `adr/2026-08-viewport-culling-onresize.md`), and `src/ui.rs` writes all four inline on the `aside.sheet`.
`assets/theme.css` keeps everything else about the card untouched: fill, border, glow, 24px/32px padding, `overflow-y: auto` — the note body scrolls inside the card — and the dim, tether and stacking order of `adr/2026-08-sheet-stacking-dom-order.md`.

**5:3 landscape** because that is the proportion of a physical index card (5 x 3 inches), which is the object the whole table screen is an argument for; landscape because a note's lines are horizontal and a portrait card at this width would be taller than most windows.
**900px** because it is the widest the card can be at 1280 — the smallest window the app is designed for, and the default viewport a headless run assumes — while still leaving a margin on each side that reads as a margin, and because at 1920 a 900px card sits comfortably inside the pane rather than filling it. The number is a ceiling, not a size: below it the card shrinks with the window, always in proportion.

**The tether is measured against the same frame.** `table::tether` takes the `SheetFrame` instead of reading the frozen constants, so the line still runs from the card's nearest edge to the card's, at every window size. A canvas card that stands *behind* the index card tethers at zero width — the existing "drawn as nothing" idiom — and at 1280 that is now the common case, since a 900px centred card covers most of the canvas. The tether stays honest: it says nothing rather than pointing through the card at a position the sheet no longer has.

**The sheet's reading column fills the card.** `.sheet-column` leaves the `min(529px, 100%)` rule it shared with `.centre-column` and becomes its own `width: 100%`: the card is already a measure, and a second cap inside it would leave the note in a narrow strip of a wide card. The logs' `.centre-column` is unchanged — its own comment loses only the sentences about the sheet sharing it. The `max-width: calc(100vw - …)` hack on the sheet's inline style goes with it: a frame computed from the observed pane is already bounded, on both axes.

## Alternatives rejected

- **A title band across the head of the card** — offered and declined: the note's own first line is its title, and a band would either repeat it or state the id, spending vertical space on a card whose whole point is that it is small.
- **Ruled lines, the way a real index card is ruled** — offered and declined: the rules would have to agree with the prose leading to look like anything but noise, and they would fight the markup model's own line boxes (`adr/2026-08-css-draws-the-markup.md`); the card reads as a card from its proportions and its glow.
- **Keeping the sheet full-height and only centring it** — the shape was the complaint, not the position; a full-height centred column is still a panel.
- **Sizing the card in CSS alone (`left: 50%; transform: translateX(-50%); width: min(900px, 100vw - 88px)`)** — the smaller diff, but the tether's arithmetic lives in Rust and would no longer know where the card stands; the line would point at x=440 while the card sat at x=510. An interface that draws a line to the wrong place is worse than one that is laid out in two files.
- **Bounding the height by clamping only the height** — breaks the 5:3 on a short window. `sheet_frame` bounds the *width* by both axes (`(pane height − 88) / 0.6`), so the proportion holds at every window size and the card never runs past the bottom of a pane that cannot scroll.
