# A modifier's own keydown is key state, never a keystroke

## Context

`d$` did nothing but move the caret to the end of the line.
The chord was implemented and tested — `src/vim.rs` dispatches `$` to `Motion::LineEnd`, `run_motion` routes it through `operator_motion` when a verb is pending, and the operator × motion matrix asserts the exact span it cuts.

The webview sends a keydown for the modifier key *itself* before the character it modifies, and `Key::Shift` is a real variant.
Normal mode's catch-all read it as an unbound key and called `reset()`, so the pending `d` was gone before `$` arrived.
`finish_prefix` had the same hole: a modifier keydown is not a `Key::Character`, so it aborted any pending prefix.

Every chord whose *continuation* key needs a modifier was broken — `d$`, `di"`, `da(`, `d^`, `dF`, `dT`, `cs"'`, `ys(` — on a keyboard where `$` is Shift+4 and the brackets and quotes are shifted digits.
Chords whose *first* key is shifted (`D`, `C`, `V`, `S`, `R`, `A`, `I`, `O`, `P`, `X`, `Y`) always worked, because the reset landed on empty state.
That asymmetry is why the bug survived phases 2–5 of v2.

The headless tests never saw it: `feed` maps each grapheme of `"d$"` straight to `Key::Character` with `Modifiers::empty()`, so the harness could not spell the modifier keydown the real webview sends.

## Decision

`Vim::handle` returns before anything else for `Shift`, `Control`, `Alt`, `AltGraph`, `Meta` and `CapsLock`.
The grammar never sees a modifier key, in any mode, so no pending operator, count or prefix can be reset by one.

- The guard sits **before** the ctrl/meta block, so all six read as one rule. `Control` and `Meta` already returned `Pass` through that block — their own keydown sets the modifier flag — so hoisting them changes nothing for them.
- It returns **`Pass`, not `Swallow`**: `Swallow` makes the sink call `prevent_default()`, and a prevented bare modifier keydown cuts the legs from under native shift-selection and the GTK dead-key machinery the owned-caret widget depends on (`adr/2026-08-hidden-ime-sink.md`). `keymap::action` answers `None` for it, so `Pass` bubbles inert.
- A test helper (`feed_keys`) takes `&[Key]`, so a test can interleave the modifier keydowns the string form cannot spell. Every chord test that matters now has one.

## Rejected

- **Guarding only normal mode's catch-all** — leaves `finish_prefix`, and any later match arm, free to make the same mistake. The rule belongs at the door.
- **Swallowing instead of passing** — see above; the IME is worth more than the tidiness of "normal mode swallows everything it does not act on". A modifier is not an unbound key; it is not a key.
- **Filtering in `src/ui.rs` before the grammar is called** — the grammar is the thing with the invariant, and it is the part that is headlessly testable. A UI-side filter would be untested by the vim suite.
