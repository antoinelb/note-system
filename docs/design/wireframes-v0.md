# Note system v0 wireframes — full description

Source: `Note system v0 wireframes.dc.html` (beside this file), exported 2026-07-25 from the claude.ai/design project "Six v0 architecture proposals".
This document describes that file in full and **supersedes** `ui-spec-table-and-logs.md`.
It is in two parts: **Part I** is the design as chosen — the operative spec for v0 and v1.
**Part II** is the six-turn iteration trail that produced it — the why behind every choice, kept so decisions don't have to be re-litigated.

How the trail converged: turn 1 proposed six information architectures; the "literal card table" (1d) crossed with the "chromeless writing surface" (1c) became turn 2's table + card overlay; option 2b (free positions, tethered overlay) won; turn 3 riffed on it and settled the layout (3a table · 3e logs); turn 4 was a mood pass whose "Deep field" (4c) won; turn 5 stripped chrome, and "One line" (5b) won; turn 6 froze the states.
The deck's final note reads: *"fold 5b + these states into the spec"* — this document is that fold.

---

# Part I — the design

## The two screens

1. **The table** — a pannable canvas of freely positioned cards with link edges drawn between them. Where knowledge lives.
2. **The logs** — daily, weekly and season notes on one screen. Where time lives.

Separate screens, one icon each way; time notes never appear on the table.
Dark is the main mode; light mode is a first-class sibling, not an afterthought.

## Chrome — "one line" (5b)

The only chrome in the whole app is a single top line and one number:

- Two 14×14 stroked icons, left-aligned: a scattered-rectangles **table** icon and a **calendar** (logs) icon.
  The current screen's icon is lit (bright ink), the other dim (muted ink).
  No text labels, no chips, no borders, no keyboard hints anywhere.
- Right-aligned: the **open-loops ember** — the count (e.g. "6") in 10px monospace, amber.
  **Zero loops = zero indicator**: the number simply isn't there. Absence is the reward.
- Top line padding in the mockups: 10px 18px (dark table), 9px 18px (light logs).

The writing sheet keeps exactly two lines of its own chrome: the in-note meta line at the top, and a footer showing only backlinks ("← 2").
Everything else — zoom, palette, link insertion, escape — is a keystroke.

## Palette — "Deep field" (4c, lightened in 5)

Indigo void; link edges drawn as constellations with star nodes; cards float like distant objects.
The one warm element is the amber ember/caret.

| Role | Dark | Light |
|---|---|---|
| Screen background | `#0d0b18` | `#f5f4f9` |
| Frame / card border | `#2a2545` | `#dbd8e6` |
| Base ink | `#c9c4dd` | `#45415a` |
| Bright ink (titles, selected, lit icon) | `#e4e0f2` | `#45415a` (inverted `#f5f4f9` on fills) |
| Muted ink (meta, labels, inactive icon) | `#6f6a8c` | `#8b87a0` |
| Faintest ink (tags, empty calendar days) | `#4a4566` / `#332c52` | `#a5a1b8` / `#d5d2e0` |
| Body prose | `#b4aecb` | inherits `#45415a` |
| Card fill | `#151226` | — |
| Capture card fill | `#100e1e` at opacity .8 | — |
| Writing sheet fill | `#171331` | — |
| Hairline dividers | `#1b1730`, `#332c52` | `#e5e2ee`, `#d0cdda` |
| Real link edges (solid) | `#332c52` | — |
| Edge node dots / selection border | `#5c538a` (focused node `#8f83c4`) | — |
| Proposed link (dashed edge, hollow star) | `#8f83c4` | `#8478b8` |
| In-text link spans | `#a99ad9` | `#6b5fa8` |
| Ember / caret amber | `#d9b06a` | `#b08a4a` |
| Star-field dots | `#3d3660`, `#332c52` | — |
| Sheet glow | `0 0 100px rgba(92,83,138,.18)` | — |

Type bars (3px solid left border on cards, dark mode): person `#a0765f` · source `#83875e` · claim `#6a94a0` · concept `#7d78a0` · project `#9a7d95` · idea `#6a76a8`.
Capture bar is grey `#4a4566`; generated has **dashed** borders (`#443d6b` edge, `#6f6a8c` bar) instead of a hue.
These are the muted dark-mode descendants of turn 1's reference hues (Part II, 1a); the full eight-type reference also defines organisation and personal.

## Typography

- App chrome: `ui-sans-serif, system-ui, sans-serif`.
- All metadata, labels, hints, rail and calendar text: `ui-monospace` at 9–10px; type labels and the month header are 600-weight, letter-spacing .09em, uppercase.
- Note prose: `'EB Garamond', Georgia, serif` — body 15px/1.75; sheet title 600 26px/1.2; logs day title 600 24px.
- Card titles: 600 12px/1.3 sans.

Note bodies are rendered typst — the app never restyles them; the wireframes' serif prose stands in for the template's own output.
UI strings are english; note content keeps its own language (the mockups are french).

## The table screen

**At rest (state 6a).**
Canvas on the void background with a faint star field (5–6 one-pixel radial-gradient dots).
Cards are absolutely positioned rectangles (~170–180px wide, padding 9px 11px): uppercase type label above a title.
Positions are persistent and only change when the user moves a card.
Real links are solid `#332c52` edges with small `#5c538a` node dots where edges meet cards.
A **proposed link** is a dashed `#8f83c4` edge with a hollow star (stroke-only circle) at its midpoint — visible but quiet.
Note kinds at a glance: permanent = filled card + hue bar; **capture** = dimmer fill, grey bar, muted title, age in its label ("capture · 3 d"); **generated** = dashed border all round.

**Writing (state 6b).**
Clicking a card opens the **writing sheet**: a tall panel (mockup: from x=440 to the right area, top/bottom inset 44px, width is the `sheetW` knob) with the sheet fill, border, and a wide soft glow.
The rest of the table dims under a `#0d0b18` overlay (opacity is the `dimOpacity` knob); the selected card stays above the dim with a brighter `#5c538a` border, and a lit edge runs from it to the sheet — the tether that keeps place legible.
Sheet content, top to bottom: meta line ("atomic-notes · claim · from smart-notes · #méthode", 9px mono, hairline underneath), serif title, serif body with in-text links, amber caret.
At the end of the page, a **proposed-link line**: above a dashed hairline, a hollow circle marker, "proposed · evergreen-notes → this note", and right-aligned the only hint in the app: "enter accept · x dismiss".
Proposals never interrupt — one dashed line at the end of the page, and accepting means the user writes (ghost-text philosophy from 1c/2b).
Footer: backlinks only ("← 2").
Escape puts the card back where it was.
6b also shows the zero-loop state: no ember anywhere in the top line.

## The logs screen

Three panes, no tabs (3e's rail + 3c's month grid, in 5b's chromeless language).

**Left — time rail** (210px, hairline right border, 10px mono).
One list where indentation alone carries scale: season rows flush ("2026-summer · season"), weeks indented 10px ("2026-w30 · week"), days indented 20px ("2026-07-23").
No boxes, no background fills: the selected day is simply **bold bright ink**; everything else muted.
Kind tags right-aligned in the faintest ink.

**Centre — the selected note**, rendered.
Meta line breadcrumbs all three scales: "2026-07-23 · daily · w30 · summer 2026".
Serif title and body; a **"captured today"** block under the day note lists that day's captures and generated notes as plain mono lines ("capture-articles-zettel · still open", "digest-smart-notes · generated") — the day gathers what happened in it.
A proposed link can wait at the end of a day note exactly as on the table (6e).

**Right — jump panel** (330px): a month grid, the only calendar in the app.
Header "july 2026"; grid is a 26px week-number gutter + 7 day columns, 10px mono, weekday header "m t w t f s s".
Day-cell encoding: **bright ink = a note exists, faint ink = empty**; the selected existing day is a filled pill (dark fill, inverted text); a selected **empty** day is outlined, not filled — *selection ≠ existence* (6d).
Week numbers are clickable scales too; below the grid, a season row ("summer · spring · winter", current lit).
Months page by scrolling — no ‹ › buttons, no chips.

**Empty day (state 6d).**
Selecting a day with no note shows the rail entry dim + italic and one centred serif line: "no note for july 24 — press enter to start one from the template" (the word "enter" set as a key).
Only enter creates the file; nothing is created by navigating.
Empty is honest: no ghost template, no pre-filled headings.

## Open knobs

The deck leaves four values as live tweaks, deliberately unfixed — pick by feel once it runs:

- `sheetW` — writing-sheet width.
- `dimOpacity` — how much the table dims behind the sheet.
- `seasonDisplay` — whether the season scale shows at all (rail rows, calendar legend, meta breadcrumb segment).
- `noteDisplay` — the designer annotations baked into the mockups; not app UI.

Also open: the table at "titles" zoom with a dense (~30-card) population, and the open-loops screen in this visual language — both named in the deck's final "try next" and not yet drawn.

## Implementation notes

- The mockups use odd spacing values (9px, 11px, 18px); the project rule is spacing in multiples of 4 — normalize at implementation (e.g. 8/12/16/20) and keep proportions, not pixel values.
- The wireframes are the **v0 + v1 target UI** (see `adr/2026-07-two-screens-table-and-logs.md`): the logs screen is v0, the table is v1.
- Two zoom levels on the table (titles ⇄ bodies, from turn 2); in 5b they live behind keystrokes, not chips.

---

# Part II — the trail

Six turns; each heading quotes the deck verbatim.
Options on the chosen path are marked **[kept]**.

## Turn 1 — "v0 app shell — six information architectures"

**1a — shared reference (not a screen).**
The type-colour system: one hue, one chroma per permanent type — person `#c98a72`, organisation `#c9a86a`, source `#a8b06a`, concept `#6fb08c`, claim `#6fa8b0`, idea `#7f96c9`, personal `#a888c9`, project `#c988ae`.
Non-permanent kinds get no hue, only value: capture grey ("grey until summarised"), generated dashed outline ("outline, disposable"), time ink ("never on the canvas"), typeless a white dot with an alert ring ("debt marker").
Defines the three semantic zoom levels: far = coloured dots with edges; mid = title cards with a 3px type bar; close = rendered typst with the real `template.typ` styling ("the app should not invent a second visual language for note bodies").
Annotation: "8 permanent hues is a lot… chrome stays grey."

**1b — "Three panes, by the book — tree · list · editor, debt in a bottom drawer."**
Classic Obsidian-like shell: category/type/tag sidebar, note list, hybrid editor pane (active block = raw source bordered in the alert colour, every other block cached SVG), backlinks/outgoing footer with dangling marked, open-loops drawer at the bottom.
Rejected as the shell, but its hybrid-editor block treatment and link footer survive as editor vocabulary.

**1c — "Chromeless writing surface — the page is the app, everything else is a palette." [kept: philosophy]**
A centred page, floating id/type chips, loop count as a dot + number in a corner, margin backlinks ("margin notes, not a panel"), a bottom command palette (jump · create · capture · today), and **ghost-text link suggestion inline with a tab hint** — "sits in the sentence you are already writing (v3, but the layout must leave room for it now)".
This option's chromelessness and ghost-text became the writing sheet's DNA.

**1d — "Literal card table — physical fiches from v0, list view is a drawer of cards." [kept: metaphor]**
Skeuomorphic paper fiches on a tan felt, slightly rotated; the picked-up card lifts, straightens and shadows; debt as a pinned index card; zoom chips dots/titles/bodies.
The table metaphor won; the skeuomorphism (texture, rotation) was flattened in turn 2.

**1e — "Journal spine — today is home, permanent notes are reached through what you wrote."**
Left date spine, centre daily note, right rail of "mentioned today" ("a consequence of the left, never a file browser").
Rejected as home, but its time-first thinking prefigures the logs screen.

**1f — "Dense workbench — split source/render, tabbed panels, everything keyboard-labelled."**
IDE-like: file tree, tabs, always-visible source+render panes ("no mode switch: source and render are always both true"), status bar with vim-style INSERT.
Rejected: too much chrome for the mood the project wants.

**1g — "Debt-first — home is the open-loops board; notes open as a slide-over."**
Kanban of captures/dangling/meta anomalies; "writing the summary is the whole promotion — no accept button anywhere"; "malformed meta is data, per the ADR".
Rejected as home; its no-accept-button principle and loops taxonomy stand.

## Turn 2 — "Table + card overlay — 1d × 1c, flat straight cards, two zoom levels (titles ⇄ bodies), debt as a small widget, logs as a separate UI"

All options: flat straight cards (no texture, no rotation), two zoom levels, loops shrunk to a widget, logs split into their own UI.

**2a — aligned grid, overlay grows in place** from the clicked card's slot ("the surrounding grid never moves").
**2b — "Free positions, tethered overlay — the sheet opens beside its card with a line back to it." [kept]**
Free canvas, edges drawn in SVG, a black tether line from the open card to the sheet, ghost-text `#l("plain-files")` + tab in the sheet; "release and the sheet snaps back into the card".
This turn also moved the type colour from the card's top edge to a **left bar**.
**2c — type lanes**: colour lives in the band label, cards white; the lane gutter stays visible beside the overlay.
**2d — ink only**: no type colour at all, type is a word; hairline sheet with a keystroke dock; logs as a pure monospace ledger ("lines, not cards").
**2e — tinted fill, dense**: colour carries the whole card; the overlay is the same card enlarged; "same tint language… so the two screens feel like one product".
**2f — link-graph table**: edges always drawn; opening a card dims the table but keeps its own edges lit ("you read its neighbourhood while writing"); loops shown *on* the graph as a ghost card ("does not exist").
2f's dimmed-table-lit-neighbourhood idea folds into the chosen design's tether + dim.

Logs experiments in this turn: ledger list, day spine, month grid, week columns, stream — the day-spine and month-grid seeds both survive.

## Turn 3 — "Riffs on 2b — bigger writing sheet, dimmed table, plus a logs screen holding daily · weekly · season together (open loops left out for now). Tweaks panel drives sheet width, dim, card colour, annotations, season band."

The `{{ }}` knobs (barW, dotDisplay, dimOpacity, sheetW, noteDisplay, seasonDisplay) date from this turn.

**3a — "Tether kept, sheet doubled — logs are three columns: day · week · season, calendar in the rail." [kept: table]**
Near-full-height sheet, dim in the paper colour above everything except the tether and sheet: "the tether is the only thing left undimmed — position stays legible, nothing else competes."
**3b** — centred sheet, dark scrim, origin card floats above the dim; logs as nested one-line bands ("the wider scales are one line each until you click them").
**3c** — sheet docked right two-thirds, table stays as a dim sliver; **logs are calendar-first with a week-number gutter** ("the week column is clickable too — one calendar for all three scales"). **[kept: the month grid, adopted in turn 5]**
**3d** — table dims to outline ghosts; full-height page ("writing is a page, not a popup"); logs as three nested bands where "the week row *is* the calendar".
**3e — "Sheet keeps the card's header, table washes out — logs on a vertical time rail." [kept: logs]**
The time rail: "indentation carries the scale: season ⊃ week ⊃ day, one rail, no tabs"; day meta breadcrumbs all three scales; jump panel on the right.
(The card-header-as-sheet-header idea was dropped in later turns — the sheet shows the note's own title.)
**3f** — minimap while writing ("place without detail"); logs as three horizontal lanes with a year strip.

The layout pick after this turn: **3a's table · 3e's logs**.

## Turn 4 — "Mood pass on the chosen layout (3a table · 3e logs) — calm, nocturnal, Nils-Frahm-quiet. Dark is the main mode; each option pairs it with a soft light mode and a palette strip."

Five complete palettes over the same layout:

- **4a Nocturne** — blue-black night `#0b0e15`, faint stars, amber ember `#d9a86a`; light mode "dusk paper, no white anywhere" `#eeece6`. "Only warm thing on screen: the cursor and the loop counter — everything else is moonlit."
- **4b Moonstone** — warm charcoal `#131312`, lamplight gold `#cbb47e`, dotted-underline links, no stars; light mode "fog grey" `#e9e7e3`. "No blue: the dark is warm charcoal, like a room lit by one lamp."
- **4c Deep field [kept]** — indigo void `#0d0b18`, **constellation edges with star nodes** (`#5c538a` dots, `#8f83c4` at the focused card), amber `#d9b06a`; light mode "pale lavender-grey, same constellation lines in silverpoint" `#edebf2`. "Links are constellations: a node star where edges meet a card."
- **4d Embers** — brown-black hearth `#16110d`, candle-amber ink `#d98f4a`; "everything sits in one warm hue family — the calm comes from never leaving it."
- **4e Still water** — slate-teal `#0b1214` with an aurora gradient at the horizon; frosted-glass translucent sheet (`backdrop-filter: blur(6px)`): "the dimmed table shows through the sheet — place stays present, softly."

## Turn 5 — "Refining 4c — lighter light mode, the jump panel swaps to 3c's month grid, and three degrees of removing chrome."

All three reuse the Deep-field palette; the light mode lightens from `#edebf2` to `#f5f4f9`; the logs jump panel becomes 3c's month grid ("no chips, no ‹ › buttons: months page by scrolling"); the focused card's label drops "· open".

- **5a Bare** — no bars at all; the ember is a lone dot + count bottom-right; the sheet is only the note and its meta line; logs and zoom live behind ⌘K. "No header, no footer, no buttons — the ember dot is the only chrome left."
- **5b One line [kept]** — two small icons (table · calendar) and the ember count are the only chrome; current place lit, the rest dim; sheet footer shows only backlinks ("← 2"). "Words, not buttons — no borders, no chips, no kbd hints anywhere." The logs rail loses its boxes too: "indentation and one bold entry carry everything."
- **5c On demand** — zero chrome at rest; mouse movement fades faint word-controls in at the edges, writing dims them to nothing ("controls exist only while the hand moves — at rest the screen is just night and notes"); logs panes lose even their dividing borders ("whitespace does the separating").

## Turn 6 — "States of 5b — both screens at 1120×600, english UI strings (summer · spring · winter), all note kinds (capture, generated), proposed links, and the rest / empty / zero-loop states."

The freeze — fully described in Part I.
The five states: **6a** table at rest with all note kinds and a proposed link; **6b** writing with all loops closed (no ember) and a proposed-link line under the text; **6c** logs, dark, day selected, with the "captured today" block; **6d** logs, empty day ("empty is honest: no ghost template"); **6e** logs, light mode, with a proposed link waiting.
Deck's closing "try next": "fold 5b + these states into the spec" · "show the loops screen in this language" · "table at titles zoom, dense (30 cards)".
