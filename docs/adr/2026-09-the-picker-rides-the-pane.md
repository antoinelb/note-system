# The in-column overlays ride the foot of the pane, not the foot of the note

> Amended by `adr/2026-09-the-sink-is-the-one-keyboard-socket.md`: the picker no longer grabs the focus, so `node.focus()` scrolls nothing; the sticky placement decided here is what keeps it at the pane's foot.

## Context

"Adding a link makes the cursor disappear" — from the user's todo.

The link picker renders inside the reading column, after every block of
the note (`src/ui.rs`, `{blocks_view()}` then `{picker_view()}`, on the
logs and inside the sheet alike), and its query field asks for focus in
its own `onmounted`:

```rust
onmounted: move |event| async move {
    let _ = event.set_focus(true).await;
},
```

`dioxus-desktop`'s `set_focus` is `window.interpreter.setFocus(id, true)`,
which is `node.focus()` — and `focus()` scrolls its target into view.
Sitting past the last block of a note taller than the pane, that grab
scrolls `.centre` (or, on the table, `.sheet`) all the way down to the
picker. The caret's own line goes off screen the instant Ctrl+L is pressed
or the second `[` is typed, and stays there for as long as the picker is
up.

Whether it ever comes back depends on the caret span remounting, because
`settle_caret` — the only thing in the app that scrolls the reading pane
(`adr/2026-08-scroll-anchor-is-consumed-once.md`) — runs from that span's
`onmounted`:

- **Accepting** moves the caret past the link it just wrote, so the span's
  key `caret-{head}-{nonce}` changes, the span remounts, and `Nearest`
  pulls the pane back. The note has still jumped down and back with no
  keystroke asking for either move (AIR LAY-2).
- **Escape** changes no editor state at all. No remount, no scroll: the
  pane stays at the note's foot and the caret is simply gone until the
  user moves it.

Measured in a headless X session at 1400×900 against a 30-line day note,
screenshots before / during / after: the pane scrolls ~700px on the
picker's mount, and after Escape the caret is nowhere on screen.

This is the same trap `adr/2026-09-the-sink-outlives-the-active-block.md`
named one day earlier for the IME sink — "`node.focus()` scrolls its
target into view, and only the caret's own mount may scroll the pane" —
and the same one `adr/2026-08-command-palette-overlay-shape.md` answered
for the palette with `position: fixed`, "because a summonable palette
buried below a long note is unreachable". The picker was left in the
scroll flow and pays it in the pane's scroll instead of in reachability.

## Decision

**`.link-picker` is `position: sticky; bottom: 0` with an opaque ground.**

It stays in flow — the column still reserves its space at the note's foot,
so nothing else moves — but it can never fall below the fold, so the focus
grab finds it already in view and scrolls nothing. The note does not move
when the picker opens, and there is nothing to restore when it closes,
whichever way it closes.

The ground is `var(--bg)` because a stuck box draws over the lines it
rides above; inside the sheet it is `var(--sheet-fill)`, the card being
its own raised surface (`adr/2026-09-the-sheet-is-an-index-card.md`).

The rule is shared with the `/` prompt and the ex line, which are the same
box by class and had the same defect for the same reason; the template
picker already wears `.command-palette` and was never in the flow.

## Rejected

- **Re-anchoring the caret when the picker closes** (bump the scroll nonce
  in `accept` and `close_picker` so the span remounts and `settle_caret`
  scrolls back). Two lines of Rust and no visual change, but it only mends
  the end state: the note still leaps to its own foot the moment the
  picker opens, and the line the link is being written into is invisible
  while its id is being typed — which is the half of "the cursor
  disappears" the user actually watches happen. It would also put a
  second scroller beside the caret's mount, which the scroll-anchor ADR
  keeps to one.
- **The palette's `position: fixed`**, at `top: 96px`, 480px wide. It
  removes the scroll too, but it moves the picker out of the reading
  column into a floating box the width of the palette, a redesign of a
  shipped surface for a bug fix. Sticky is the in-column form of the same
  answer and leaves a short note's picker exactly where it has always
  been.
- **Restoring the pane's scrollTop in the picker's own `onmounted`**,
  around the grab. It needs the scroller's handle and two more eval round
  trips to undo one the app did not want, and it would fight the grab on
  every re-render rather than remove the reason for it.
- **Rendering the picker above the blocks.** The grab would then scroll to
  the note's *top* instead of its foot: the same jump, upward.

## Ceiling

`make test` renders markup, never layout, so no headless test executes
this rule; the e2e harness asserts on files and the index, never on pixels
(`adr/2026-08-headless-x11-e2e.md`). The regression guard is
`the_picker_is_taken_out_of_the_reading_column_scroll_flow` in `src/ui.rs`,
which reads the stylesheet the page inlines
(`adr/2026-08-theme-css-inlined.md`) and pins both halves of the
invariant: the box is in the scrolling column after the blocks, and the
rule takes it out of that column's scroll flow. The scroll itself was
verified by eye, in the headless X session the harness starts.
