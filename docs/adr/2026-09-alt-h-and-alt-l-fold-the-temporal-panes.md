# The rail is 176px wide, and Alt+H / Alt+L fold the two temporal panes

## Context

`todo.md` held three items about the logs screen's side panes, retired into `docs/plans/2026-09-02-temporal-panes.md` on 2026-09-02: the note's width never exceeding what is available (landed with the fluid reading column), the rail narrower "to fit dates with only a bit of padding", and a way to fold both panes.
The rail was 208px for rows whose widest — `2026-summer` with its `season` tag — ends at 192px on a headless screenshot; the jump panel is 248px for a month grid; on a laptop the two together cost the reading column a third of the window.

## Decision

**The rail is 176px**: the widest row the fixture vault draws plus the 16px gutter each side, measured once on a headless screenshot rather than guessed.
Spacing stays in multiples of 4.

**Alt+H folds the rail and Alt+L the jump panel**, both toggles, both session-only like the settings overlay's knobs.
A folded pane keeps its hairline and nothing else, so the fold reads as a line where the pane was and the centre takes the room; nothing else on screen moves.
The chords are taken in three places that share one toggle: the logs pane itself (the empty day, where no sink holds the keys), the sink in normal mode (the grammar would otherwise swallow the chord inert before the pane saw it), and two palette rows, "fold rail" and "fold jump panel", offered on the logs only.

**Insert mode is untouched.** On a French layout AltGr characters carry `alt`, and `keymap::action` keeps every alt character insertable for that reason; the fold arm sits behind normal mode, and `keymap::fold` answers only for the letter itself with plain Alt — a `{` or `«` that carries `alt` folds nothing anywhere.

## Alternatives rejected

- **Folding from insert mode too** — the one keystroke a French vault types most, an AltGr composition, would race the fold; leaving the note is one Escape.
- **`display: none` for the fold** — the hairline is the affordance that says a pane is folded rather than gone (AIR: nothing disappears without a trace).
- **Persisting the folds** — the settings overlay's knobs are session-only for the same reason: no persistence file exists, and a fold is a mood, not a configuration.
- **Ctrl+H / Ctrl+L** — the webview owns some Ctrl letters and the app spends most of the rest; Alt with a letter was free on both screens.
