# A platform key is not a keystroke: it never aborts a pending chord

## Context

`d^` still did nothing after two fixes, while `^` alone worked and `d$`, `di"`, `dF` all worked.
That asymmetry is the whole clue: `^` alone has no pending grammar to lose, so a reset in the middle of the sequence is invisible there and fatal for `d^`.

`^` is a dead key on a French layout, so `d^` reaches the sink as three events — `d`, the dead key's own keydown, then the composed `^`.
The middle one was taking the `d` with it.

Normal mode's catch-all treated every key it did not bind as an aborting keystroke:

```rust
_ => { self.reset(); Outcome::Swallow }
```

Only non-`Character` keys ever reach that arm — `Character`, `Escape` and the arrows are all handled above — so its whole population is platform keys: dead keys, F-keys, media keys.
And one more that is not obvious: Dioxus's keyboard deserializer ends with

```rust
std::str::FromStr::from_str(&self.key).unwrap_or(Key::Unidentified)
```

so any key string it cannot parse silently becomes `Key::Unidentified`.
The sink's `Key::Dead` guard cannot catch that, and on this keyboard the `^` key demonstrably does not resolve cleanly — `tao`'s GTK path prints `Couldn't get key from code: BracketLeft` on every press.

`finish_prefix` had the same hole: any non-`Character` key was read as a *wrong answer* to a pending prefix and killed it, rather than as no answer at all.

## Decision

- **Normal mode's non-`Character` catch-all swallows without resetting.** Visual mode's already did (`_ => Outcome::Swallow`); the two modes now agree, and a chord cannot lose its verb to the layout machinery whatever the webview calls the key.
- **A pending prefix waits through a platform key** instead of treating it as a mismatched answer. Escape still kills it outright — that is the ladder's pending rung and stays explicit.
- **An unbound *character* still aborts**, in `normal_character`, exactly as vim does: `dz`, `dy`, `diz`, `dfZ` are still nothing. The distinction is character versus platform key, not bound versus unbound.

The cost is that a genuinely unbound platform key — F5 mid-chord — now leaves the chord armed instead of cancelling it. Escape is the cancel, and it always was.

## Rejected

- **Adding `Key::Unidentified` beside `Key::Dead` in the sink's guard** — fixes this keyboard and waits for the next one. The grammar, not the widget, is where the invariant belongs, and it should not have to enumerate the ways a platform can fail to name a key.
- **Reading the raw `event.code()` to recover the dead key** — already rejected twice, in `adr/2026-08-hidden-ime-sink.md` and `adr/2026-08-normal-mode-compositions-reach-the-grammar.md`: it reimplements the layout tables GTK owns.
- **Keeping the reset and special-casing composition state** — this was the previous attempt (`adr/2026-08-composition-ownership-is-mode-independent.md`), and it was necessary but not sufficient: it guards the *commit* keystroke, while the dead key's own keydown arrives before any composition has started, so no composition flag can be up yet.
