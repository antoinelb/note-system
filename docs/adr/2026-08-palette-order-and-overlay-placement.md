# The palette lists commands alphabetically and drops one row; every overlay renders above both screens

> Amended by `adr/2026-09-the-sink-is-the-one-keyboard-socket.md`: an overlay no longer grabs the focus in its `onmounted`; its keys reach it from the sink. The placement stands.

## Context

Todo 21 reported the notices pane not closing on a click; todo 24 reported the open-loops command doing nothing from the table screen; todo 23 asked for the palette's capture-clipboard row to go and the rest to sort alphabetically. All three turned out to share one root cause and one fix shape: an overlay that is not rendered at `Shell`'s top level, or not wired with its own focus and dismissal, behaves differently depending on which screen summoned it.

`2026-08-settings-overlay.md` already named the defect once — "a deliberate correction of the loops list's mistake" — but only moved the *settings* overlay itself out of the `Screen::Logs` block; the loops list it was describing stayed exactly where it was, still broken from the table.

## Decision

**Every overlay renders once, at the top level of `Shell`, beside `Chrome`.** The open-loops list (`div.loops-list`) moves out of the `if screen() == Screen::Logs` block and stands next to the notices and settings overlays, guarded only by its own condition (`loops_open() && !loops.read().is_empty()` — nothing renders at zero loops, unchanged). The ember and the palette's "open loops" command both flip the same `loops_open` signal, so both now reach the same overlay from either screen. The floating box itself comes from the markup, not from a duplicated rule: the overlay's div wears `class: "command-palette loops-list"`, so it inherits the palette's fixed-position box from the one `.command-palette` rule, and `theme.css` keeps no `.loops-list` box declaration of its own — the class only carries the list's typography (`.loops-head`, `.loops-line`). A top-level sibling needs the fixed-position treatment or it pushes the flex-column app layout around instead of floating over it; sharing the class is how it gets it.

**Every overlay is self-contained: it grabs focus, and it closes on its own Escape and on a click anywhere in it.** The notices pane was the one overlay without this — a bare `div` with no `tabindex`, no `onmounted`, no `onkeydown`, no `onclick`, so a click never reached it and Escape only worked when some other element (the pane, or nothing) happened to hold focus. It now gets exactly the treatment every sibling overlay already has (the create overlay's input, the settings overlay's own div): `tabindex="0"`, an `onmounted` that grabs focus, an `onkeydown` closing on `Key::Escape`, and — new, because notices and loops hold no other interactive row — an `onclick` on the whole box that closes it too, since nothing inside is worth a narrower target. The loops list gets the identical four. The app-level `Key::Escape if notices_open()` rung was already on both panes' ladders (the logs `keyboard` closure and the table `table_keys` closure) and stays on both as the defence-in-depth fallback `2026-08-settings-overlay.md` already established for settings. The logs ladder already had a `Key::Escape if loops_open()` rung too; the table ladder did not, because the loops overlay had never been reachable from the table screen before this change — that rung is added to `table_keys` here, so both panes now carry both fallbacks. Neither rung is the *only* path to closing; each is a fallback behind the overlay's own `onkeydown`.

**The palette lists its rows alphabetically by label**, replacing the registration-order layout `2026-08-palette-birth-command-list.md` shipped with. `palette::COMMANDS` is reordered so the array itself is the sorted order — `filter` still just walks it in place, so alphabetization falls out of the data rather than a sort step at query time. A registry test (`the_registry_is_alphabetized_by_label`) pins this so a later append cannot silently regress it back to registration order.

**`capture clipboard` is dropped from the registry**, `CommandId::CaptureClipboard` deleted along with it. The raw `Ctrl+Shift+V` handler in `Shell`'s `keyboard`/`table_keys` closures is untouched — the chord still captures the clipboard exactly as before, it simply has no palette row naming it.

## This amends `2026-08-palette-birth-command-list.md`

That ADR's stated invariant was explicit: "the palette lists all nine user-invocable commands... it is the complete named surface of the app, not just an index of its chords." Dropping `capture clipboard` breaks that invariant on purpose — the palette is no longer an exhaustive index of every command or every chord. `Ctrl+Shift+V` now answers to a chord with no palette row, the first such case. The birth ADR's completeness *tests* (the registry-against-chords audit) stay, narrowed to the chords that do have rows; the birth ADR's prose claim of completeness does not survive this change and is superseded here.

## Amended by `2026-09-palette-orders-by-usage.md`

The alphabetical order stated here is now the *tie-breaker*, not the order: the palette lists commands by how often each has been run, descending, and equal counts fall back to the alphabetical array this ADR established.
The registry itself stays alphabetical and `the_registry_is_alphabetized_by_label` still pins it — that is what makes the fallback free — but `filter` now ends in one stable `sort_by_key`, which is precisely the "sort at query time" the first rejected alternative below turned down.
That rejection stood while the order was a fixed property of the array; it does not survive an order that depends on state the array cannot carry.

## Rejected

- **A `sort_by_key` at query time instead of reordering the array** — would keep `COMMANDS` in registration order in the source (arguably easier to append to) but pay a sort on every keystroke for a list that is small, bounded, and rarely changes; reordering the constant once is free and keeps `filter`'s "walks the array in place" contract intact.
- **Keeping `capture clipboard` as a row that copies the chord's own guard (block always active) instead of removing it** — the row never did anything the chord itself didn't already do faster; the plan explicitly asked for the row gone, the chord kept.
- **Giving the notices/loops overlays a narrower click target (e.g. only a close button) instead of the whole box** — neither overlay has another clickable element worth distinguishing from "dismiss"; a whole-box click matches how a reader already dismisses a transient list, and keeps the change to one `onclick` per overlay instead of new markup.
