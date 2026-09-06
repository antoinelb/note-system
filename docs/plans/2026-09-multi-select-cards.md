# Selecting several cards and moving them together — four designs

Status: proposal, no code.
Written against `todo.md`'s two open table items: "notes should move so they never overlap (plus a small padding)" and "a way to select multiple cards to move together".
The no-overlap rule (neighbours yield on drop, 8px padding) is assumed to land in parallel and is treated here as a constraint, not as work.

## What the table already owns

Listed so no design below collides with a gesture that exists.

Keys the table screen answers, all in `table_keys` (`src/ui.rs:3336`):
Escape (closes the loops overlay, else acknowledges the status notice), Shift+Escape (closes the sheet), Ctrl+L, Ctrl+T, Ctrl+P, Ctrl+N, Ctrl+D, Ctrl+Shift+D, Ctrl+comma, Ctrl+1, Ctrl+2, Ctrl+Enter, Ctrl+=, Ctrl+-, Ctrl+F, Ctrl+Shift+F, Ctrl+O.
Ctrl+Q is answered above the screens, in the shell (`src/ui.rs:343`).
Alt+H and Alt+L fold the temporal panes on the logs screen only (`keymap::fold`), so they are free on the table but spoken for by vocabulary.

The decisive fact: **on the bare table every bare key is free.**
`sink_keys` runs the vim grammar only when `let hosted = *screen.peek() == Screen::Logs || sheet.peek().is_some();` holds (`src/ui.rs:4018`), so with no sheet open no letter reaches the grammar and `table_keys` answers none of them.
Over an open sheet the grammar owns every bare key, and nothing below may take one back.

Mouse gestures the canvas owns (`src/ui.rs:4708` onward):
a card's `onmousedown` seeds `Grab::Card` and stops propagation; a mousedown the cards did not stop seeds `Grab::Void`, which pans; one `onmousemove` moves whichever is held, writing `positions` live on every frame; `onmouseup` compares the whole travel against `table::CLICK_SLOP` (4px, max-norm) and opens the sheet when the press was a click (`adr/2026-08-click-opens-drag-moves.md`).
There is no double-click, no context menu and no wheel handler on the table — the one `onwheel` in the file belongs to the logs' jump panel.

Colour tokens available in `assets/theme.css`, all already carrying both themes:
`--selection` (the raised card's border, the tether, the edge node dots), `--select-fill` (the same hue at 0.25 alpha, the drawn text selection), `--ember` (the one warm element, spoken for by open-loops debt), `--dim-opacity` and `.card.dimmed`'s own 0.25 opacity, `--ink-bright`/`--ink-muted`, `--hairline`, `--card-fill`.
The file's own comment on `--ink-bright` cites AIR INP-3: "every selected state carries weight as well as hue", which every design below must honour — a hue alone is not a selection mark.

---

## Design A — marquee plus Shift+click (mouse-first)

**Gestures.**
Shift+drag on empty canvas draws a marquee and selects every card whose rectangle *intersects* it (intersects, not contains, so a half-covered card counts).
Shift+click on a card toggles that card in or out.
A bare drag on empty canvas still pans, unchanged.
A bare click on empty canvas clears the selection.
Dragging any selected card drags the whole set, each member keeping its offset.

**Drawing.**
A `.card.picked` class: `border-color: var(--selection)` plus `background: var(--select-fill)` layered over the card fill — hue and fill weight, two channels.
The marquee is a `div.marquee` with `1px solid var(--selection)` and `background: var(--select-fill)`.
No count is drawn; the marks are the count.

**Moving.** Drag only — there is no keyboard half.

**Clearing.** Escape gains a rung above the notice rung and below the loops rung; a bare click on the void clears too.

**No-overlap.** On drop the whole set is offered at once and the selection is excluded from the yield candidates, so members never push each other apart; only outside neighbours yield, with the 8px padding.

**Undo.** One `undo::Intent::Move { prior: Vec<(String, Option<(f64,f64)>)> }`, identical in shape to `Intent::Arrange`, recording every moved card *and* every yielded neighbour.

**Collisions.** None, if the marquee takes Shift+drag; taking the *bare* void drag would collide head-on with the pan, which is the table's most-used gesture.

**Cost.** `src/ui.rs` gains a `Grab::Marquee` variant and three handler arms plus the selection signal and the `.picked` class in the card loop, `src/table.rs` gains a pure `marquee_hits(rect, cards)` and a `move_set` offset helper, `assets/theme.css` gains two rules, `src/undo.rs` gains the `Move` intent and its label, plus one ADR and one e2e scenario; roughly five files and no new module, the smallest of the three real designs, with the 100% gate falling mostly on `table.rs`'s pure helpers rather than on `ui.rs`.

---

## Design B — a visual mode for cards (the vim one)

This one has a prerequisite that is a feature in its own right: **the table has no card cursor today**, so a card can only be opened by clicking it.

**Gestures, normal mode on the bare table.**
`h`/`j`/`k`/`l` move a card cursor to the nearest card in that direction; `Enter` opens the cursor card's sheet; `gg`/`G` jump to the first/last card by id.
Nearest is defined deterministically and without a search: among cards whose centre lies in the direction's half-plane, minimise `primary distance + 0.5 × cross distance`, ties breaking to the smaller id.

**Gestures, visual mode.**
`v` enters visual with the cursor card selected; each `h`/`j`/`k`/`l` then moves the cursor and adds the card it lands on, so the selection is a trail, not a rectangle.
`Space` toggles the card under the cursor out of the set (the escape hatch a trail needs).
`H`/`J`/`K`/`L` nudge the whole set by 16px, the design grain's fourth multiple.
`a` arranges the selection as a cluster, reusing `arrange`'s spring pass with the selection as its scope instead of `arrange::component`.
Shift+click and the marquee from design A can fill the same set from the mouse; the set is one piece of state and the gestures that fill it are independent.

**Drawing.**
The cursor card wears `.card.cursor`: a 2px `var(--selection)` border, no fill — position without membership.
A selected card wears `.card.picked` as in design A, cursor and picked composing on the cursor card.
The chrome, which already reserves its line's height in every state, says `visual · 3 cards` while the mode stands — a mode this powerful must be legible without looking at the cards.

**Moving.** `H`/`J`/`K`/`L` nudge, `a` arranges, and dragging any selected card still moves the set.

**Clearing.** Escape clears the selection and returns to normal in one press, vim's own rule; the cursor survives, harmlessly, and a second Escape falls through to the existing notice rung.

**No-overlap.** A nudge is a drop: the same yield pass runs after each `H`/`J`/`K`/`L`, with the selection excluded from the candidates.
A held nudge would then yield repeatedly, so the nudge and its yields must land in one intent per keypress, not per frame.

**Undo.** The `Move` intent of design A, pushed once per nudge, once per arrange and once per drag.

**Collisions.** None on the bare table, where every bare key is free.
The mode must be refused while a sheet is open, because there the grammar owns `v`, `h`, `j`, `k`, `l`, `a` and `Space` outright; the guard is one `sheet.peek().is_none()` on the entry arm, the shape every table chord already wears.

**Cost.** The largest: `src/table.rs` gains the nearest-card motion, the cursor's initial pick and the selection set with its trail and toggle rules, `src/ui.rs` gains a mode signal, a cursor signal, six or seven key arms and two classes in the card loop, `Chrome` gains a mode prop beside the filter label it already carries, `assets/theme.css` gains three rules, `src/arrange.rs` gains a scope argument or a second entry point, `src/undo.rs` gains the `Move` intent, and it wants two ADRs (the cursor, then the mode) and two e2e scenarios; seven or eight files, and the coverage gate is real work because every motion arm and every refusal branch needs its own `ui.rs` test.

---

## Design C — the set is named, never picked (filter and cluster)

**Gestures.**
No per-card picking at all: two palette commands promote an existing computed set into the selection.
"select filtered" takes every card the active `table::Filter` did *not* dim, so Ctrl+F then the palette is the whole gesture.
"select cluster" takes `arrange::component` of the open sheet's card — the scope `arrange cluster` already uses — and is available on the same `context.sheet_open` rule.
The set is then moved by dragging any member, and cleared from the palette's "clear selection" or by Escape.

**Drawing.** `.card.picked` as above, and the chrome's filter label gains `· 7 selected`.

**Moving.** Drag only, unless design B's nudges land alongside.

**Clearing.** The Escape rung, plus applying a new filter, which invalidates what the old one named.

**No-overlap.** Identical to A; a cluster selection is exactly the case where excluding members from the yield candidates matters most, since a cluster is dense by construction.

**Undo.** The same `Move` intent.

**Collisions.** None whatsoever — it adds no key and no mouse gesture, only palette rows.

**Cost.** `src/palette.rs` gains two or three `CommandId` variants with their availability rules, `src/usage.rs` gains their kebab keys, `src/ui.rs` gains their handlers and the `.picked` class, `src/table.rs` gains the predicate-to-set function, `assets/theme.css` one rule, `src/undo.rs` the `Move` intent; six files but every piece is small and mechanical, and it is the only design here that could ship in an afternoon.

---

## Design D — a marking mode with a count badge

**Gestures.**
A chord — Ctrl+Space is free — enters marking mode.
While it stands a plain click on a card toggles its mark instead of opening its sheet, the void still pans, and dragging any marked card moves the set.
Escape leaves the mode and clears the marks.

**Drawing.**
`.card.picked` again, plus a persistent badge in the chrome reading `marking · 3` in `--ink-bright` on the chrome's own ground.
Not `--ember`: the ember means open-loops debt and must not learn a second meaning.

**Moving.** Drag only.

**Clearing.** Escape, one press, mode and marks together.

**No-overlap and undo.** As in A.

**Collisions.** Ctrl+Space is unclaimed on both screens.
The real cost is not a key but a rule: **this design makes a click mean two different things depending on a mode**, and mode-dependent behaviour is the failure AIR's LAY section names outright.
It is survivable only if the badge is unmissable and the mode cannot be entered by accident, which is why the entry is a chord and not a letter.

**Cost.** The smallest: `src/ui.rs` gains a mode signal, one branch inside the existing `onmouseup` click arm and one key arm, `Chrome` gains a badge prop, `assets/theme.css` gains two rules, `src/undo.rs` the `Move` intent; four files, one ADR, one e2e scenario.

---

## What all four share

Three things are the same work whichever design lands, and should be decided once.

**The moving set must be rigid against the yield rule.**
If the no-overlap pass treats a group drop as N independent drops, members shove each other and the arrangement the user built is destroyed by the move that was supposed to preserve it.
The pass must take the selection as one body: offer every member's target rectangle at once, exclude members from each other's yield candidates, and let only outside neighbours move.

**A group move needs undo, and that reopens a closed decision.**
`adr/2026-08-app-level-undo-register.md` rejected drag undo because "the gesture corrects itself by dragging back".
With neighbours yielding, that is no longer true: dragging the group back does not un-yield the neighbours, so a drag now has a consequence the gesture cannot reverse.
Whichever design lands should carry that reversal in its own ADR, and `Intent::Move` should record the yielded neighbours alongside the moved members.

**Selection must carry weight, not only hue.**
`--selection` plus `--select-fill` gives border and fill, two channels, which satisfies INP-3; a border-colour swap alone does not.
And the count belongs on the chrome's reserved line rather than floating over the canvas, so nothing on the table moves because a selection appeared.

---

## Recommendation

**Design B, with design A's two mouse gestures folded into it as the mouse half of the same selection set.**

The selection is one piece of state; the four designs differ only in which gestures fill it, so choosing B does not exclude A — it costs two handlers to add Shift+click and the Shift+drag marquee once the set exists.
B is the only design that answers the brief's first clause, keyboard-first: A, C and D all still require the mouse to move a group, and C and D require it to build one.
B also pays a debt that has nothing to do with multi-select — today a card can only be reached by clicking it, and a card cursor with `Enter` gives the table the keyboard access the logs screen has always had.
Vim is the editing idiom here, and `v` on the bare table is free precisely because the grammar declines to speak where no note is drawn, so the idiom extends without a single guard fighting another.
C is the right *first* increment if the appetite is small: it ships the selection set, `.card.picked`, the rigid-body yield rule and `Intent::Move` — every shared piece above — behind two palette rows and no new grammar, and B then arrives as gestures over state that already works.
D is the one to decline: it buys the least and is the only design that makes an existing gesture mean two things.

## Open questions

1. Should the group drag be undoable at all, given the drag-undo rejection in `adr/2026-08-app-level-undo-register.md` — or only the nudges and the arrange, with drags staying self-correcting as they are today?
2. Does the trail semantics of `v` (every card the cursor passes joins) match your intent, or would you rather `v` toggle and `Space` be the only way in, so a motion never adds by surprise?
3. What is the right nudge step — 16px as proposed, `arrange::SLOT_W`/`SLOT_H` (192/96, the ring walk's pitch), or one of each on `HJKL` and `gHJKL`?
4. Should the selection survive a screen switch to the logs and back, or die with the table view the way the pan and zoom do not?
5. Should a selection be deletable (`d` over N cards, one keypress, unconfirmed and trashless like every other delete here), or is that a blast radius the register's depth of 10 should not be asked to hold?
6. Should `a` over a selection reuse the existing "arrange cluster" command's name and undo label, or is a selection arrange a different enough act to deserve its own row?
7. C's "select filtered" is the only design that can name a set larger than the viewport — do you want a group move that includes cards you cannot see?
