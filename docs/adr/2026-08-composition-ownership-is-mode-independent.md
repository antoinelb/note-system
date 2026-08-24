# A composition owns the keyboard in every mode, and its end gets one keystroke of grace

## Context

`d^` did nothing while `^` alone worked, and `d$`, `di"`, `dF` — the other chords the modifier guard had just fixed — all worked too (`adr/2026-08-modifier-keydowns-are-key-state.md`).

`^` is a dead key on a French layout, so it reaches the grammar only as a committed composition (`adr/2026-08-normal-mode-compositions-reach-the-grammar.md`).
The keystroke that *commits* it — a space, or a second `^` — arrives at the sink as its own keydown, and WebKitGTK can send it **unflagged**: `isComposing` false, key `Character(" ")`.
Normal mode read it as a real key, found it unbound, and reset the pending `d`.
The composition then committed `^` into an empty grammar, so the caret moved to the first non-blank and nothing was cut.

The sink already had a guard for exactly this — the spike added `preview.peek().is_some()` as the third one, "what absorbs the ordering surprises the spike saw" (`adr/2026-08-hidden-ime-sink.md`).
But `preview` is set only in insert mode, because normal mode must never *draw* a preview.
One signal was carrying two different facts: *a composition is open* (input state, true in every mode) and *a preview is drawn* (render state, insert only).
Normal mode needed the first and was refused it along with the second.

The second half of the problem is WebKitGTK's doubled end, also in the spike's transcript: an **empty `compositionend` fires before the real one, with a stray unflagged keydown between them**.
A flag that drops on the empty end is already down when that stray key lands.

## Decision

- **Composition ownership is its own state, independent of the drawn preview** — a `Composing` signal beside `preview`, set in every mode. The sink's third guard reads it instead of the preview, which was only ever standing in for it.
- **The end gets exactly one keystroke of grace.** `Composing::Open` on `compositionstart`; an empty `compositionend` moves to `Composing::Closing` rather than `No`, and the next keydown is swallowed and spends the grace. A non-empty end goes straight to `No`.
- **The grace is bounded at one keystroke on purpose.** A composition that genuinely ends empty — an abort — costs the user one keypress and can never wedge the editor. A flag held until the real end would hang the keyboard whenever the real end never came.

## Rejected

- **Setting `preview` in every mode and checking the mode when drawing** — reuses a signal instead of adding one, but re-entangles input state with render state, and pushes the mode test into the render path where a miss draws a preview normal mode must not have.
- **Clearing the flag on every `compositionend`** — no hang risk, and it fixes the unflagged commit keystroke, but the stray keydown between the doubled ends walks straight through it. Tested: that ordering is the third case in `a_dead_key_behind_a_verb_keeps_the_verb` and it fails under this rule.
- **Holding the flag until a non-empty end, with no grace** — closes both orderings and wedges the keyboard on any aborted composition. Unbounded state waiting for an event that may not come.
- **Never resetting pending grammar on an unbound key** — would paper over this without a composition concept, but it also retires vim's real behaviour (`dz` aborts) which the grammar tests pin.
