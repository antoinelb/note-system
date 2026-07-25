# UI direction — the table and the logs screen

From wireframes `3a` (table + card writing) and `3e` (logs) in `Note system v0 wireframes.dc.html`.
This is the **layout direction only** — enough to build a rough version and feel it. Detail (exact sizes, colours, editor behaviour) is deliberately left open; expect it to change once the layout is on screen.

Open loops are out of scope: they appear as a counter in the corner and nothing more.

---

## Two screens

1. **The table** — the spatial canvas of cards. Where knowledge lives.
2. **The logs** — daily, weekly and season notes on one screen. Where time lives.

They are separate screens with a button each way. Time notes never appear on the table.

---

## The table

A pannable canvas of freely positioned cards, with link edges drawn between them.

- **Two zoom levels only**: titles (default) and rendered bodies. The "coloured dots" level is dropped for now.
- **Cards** are flat, straight rectangles — no paper texture, no ruled lines, no rotation. A card shows its type and title; type is a thin colour bar on the left edge, and that is the only colour in the UI.
- Positions are persistent and only change when the user moves a card.
- Top bar: the zoom switch, a button to the logs, and the open-loops counter.

### Writing on a card

Clicking a card opens a **writing sheet** — a tall, roughly 600 px wide panel that is much larger than the card.

- It opens **beside its card**, with a line tethering it back to the card.
- The rest of the table **dims** behind it; the origin card and its tether stay lit, so you never lose where you are.
- The sheet is chromeless: a thin header with the id and type, the note itself, and a footer line with link counts. Everything else is a keystroke (palette, insert link, escape).
- Escape puts the card back where it was.

The point of the whole arrangement: writing happens at the size of a page, but the sense of place stays visible around it.

---

## The logs

Three panes, one screen, no tabs.

- **Left: a time rail.** One list where indentation carries the scale — season, then its weeks, then their days. The rail is the navigation; nothing is hidden behind a mode switch.
- **Centre: the selected note**, rendered, with its scale chain (day · week · season) shown above it and clickable.
- **Right: a jump panel** — a compact month calendar where days that have a note are marked, plus week and season chips. This is the only calendar in the app.

Selecting anything in the rail or the calendar swaps the centre pane. Clicking an empty day offers to create it from the template; nothing is created just by navigating.

---

## Kept from the existing plan

- Note bodies are rendered typst — the app never restyles them, and the meta line comes from the note itself.
- App chrome is greyscale; the only colours are the type bar on cards and a single alert hue for debt and dangling links.
- Nothing blocks saving or writing.

---

## Open questions to settle while building

- Sheet width and how much the backdrop dims — pick by feel once it runs.
- Whether the rail scrolls infinitely or pages by month.
- What the card looks like at the "bodies" zoom level, given the sheet is where real writing happens.
- Whether the scale chain in the logs is enough, or the week/season notes need their own panes.

