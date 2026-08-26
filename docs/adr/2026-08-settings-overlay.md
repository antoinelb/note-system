# A settings overlay on Ctrl+,, holding the theme and the font size — session-only, no config file

## Context

`2026-07-theme-keystroke-toggle.md` put the theme behind a keystroke and explicitly rejected persisting the choice, reasoning that "a settings file for one bit is machinery nothing else needs yet". The font size later became a second session-only knob (`2026-08-one-font-size-for-source-and-render.md`), reachable by no control at all — only the palette's "toggle theme" row and a hard-coded `DEFAULT_SIZE` constant. Todo 16 asks for a settings page; the question this ADR answers is what that page is, where it lives, and whether the "no config file" stance still holds now that there are two knobs instead of one.

## Decision

**One overlay, the command-palette's own box, opened by Ctrl+,.** It reuses the palette's `.command-palette` fixed-position box (`2026-08-command-palette-overlay-shape.md`) with a `.settings` modifier class, rendered once at the top level of `Shell` — beside `Chrome`, the palette, the creator, and the notices overlay, outside both the `Screen::Logs` and `Screen::Table` blocks. This is a deliberate correction of the loops list's mistake: `.loops-list` is nested inside the `Screen::Logs` block alone, so the open-loops command does nothing when summoned from the table. The settings overlay, the notices overlay, and every future top-level overlay must render where both screens can see it.

**Two rows, both live controls, the app's first buttons.** Every existing overlay row is either a picker match (click-to-run) or, in the notices overlay, inert display; the settings overlay is the first place a click does something other than run a command or select a row. The theme row is a single button that names the *current* theme and toggles it on click, wired through the same `RootCommands::toggle_theme` callback the palette's "toggle theme" row already calls — one code path, two doors. The font-size row is a `−`/current-value/`+` stepper, moving `font_size` in `2px` steps, clamped to `12..=28`: below 12px prose is unreadable, above 28px a line stops fitting the pane at ordinary widths.

**Session-only state, still.** `settings_open` is a plain `use_signal(|| false)`, and neither the theme nor the font size gain any persistence here — closing the overlay or quitting the app forgets both, exactly as `2026-07-theme-keystroke-toggle.md` already chose for the theme alone. Nothing about this overlay changes that stance for the font size either: a page for two sub-KB numbers is not machinery worth building yet, and every relaunch is dark-at-18px until a config file earns its keep some other way. If persistence is ever wanted, it is a separate decision with its own ADR — this one only gives the two knobs a page, not a save path.

**Escape closes it two ways: the overlay's own `onkeydown`, and a rung on each pane's ladder.** The overlay grabs focus at mount (the create overlay's idiom) and answers Escape directly; the `keyboard` (logs) and `table_keys` (table) closures also gain a `Key::Escape if settings_open()` rung, above the notices and loops rungs, as defence in depth for the case focus is not on the overlay when Escape lands. `Ctrl+,` itself is threaded through the same overlay-stacking guard every other summoning chord (Ctrl+L, Ctrl+T, Ctrl+P, Ctrl+N, Ctrl+D, and the table's Ctrl+F/Ctrl+O) already carries, and `settings_open` is added to those chords' own guards in turn, so no two overlays can ever be open together.

**A palette row, `"settings"`, chord `ctrl+,`.** Listed like every other command, at its alphabetical place — todo 23's resort (`2026-08-palette-order-and-overlay-placement.md`) landed in the same change, so the registry was never in an append-at-the-end state.

## Rejected

- **A dimmed backdrop or a dedicated settings screen** — the palette's own ADR already rejected a backdrop for one floating box; a second overlay style for a two-row page would be a second vocabulary for no reason.
- **Persisting the theme and font size to a file now that there are two of them** — two numbers are still not machinery; the original reasoning was about the *idea* of a settings file, not its size.
- **Reading `prefers-color-scheme` or an environment default for either knob** — unchanged from `2026-07-theme-keystroke-toggle.md`; dark-at-18px is the app's stance on every launch.
