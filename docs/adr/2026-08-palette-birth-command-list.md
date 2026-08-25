# The palette's birth command list names every user-invocable command

**Amended by `2026-08-ctrl-t-toggles-the-todo.md`** (the `ToggleTheme` row keeps its label but loses its `ctrl+t` chord — the theme is reached from the palette alone now) **and by `2026-08-time-navigation-commands.md`** (`GoToToday` is renamed `OpenDaily`; `PreviousMonth`/`NextMonth` are removed — those interactions stay the calendar widget's alone; `OpenDaily`/`PreviousDaily`/`NextDaily` and their weekly/season siblings are added). The table below is the v1-phase-0 birth snapshot; `src/palette.rs`'s registry and its own completeness test are the living source of truth.

## Context

v1 phase 0 gives the app a Ctrl+P command palette
(`adr/2026-08-command-palette-first-in-v1.md`), and its roadmap line "the
command set at birth" is a decision to record: which commands exist, under
what names, and what happens to the ones that need context. The app today
answers seven chords (Ctrl+T, Ctrl+Q, Ctrl+Shift+V, Ctrl+L, Ctrl+Enter,
ArrowLeft, ArrowRight) and two mouse-only gestures (the ember click that
opens the loops list, the "today" button).

## Decision

The palette lists **all nine user-invocable commands**, the mouse-only two
included — it is the complete named surface of the app, not just an index of
its chords:

| command | label | chord shown |
|---|---|---|
| `ToggleTheme` | toggle theme | `ctrl+t` |
| `Quit` | quit | `ctrl+q` |
| `CaptureClipboard` | capture clipboard | `ctrl+shift+v` |
| `InsertLink` | insert link | `ctrl+l` |
| `FollowLink` | follow link | `ctrl+enter` |
| `PreviousMonth` | previous month | `←` |
| `NextMonth` | next month | `→` |
| `OpenLoops` | open loops | — (the ember) |
| `GoToToday` | go to today | — (the today button) |

- **Contextual commands are hidden, not disabled.** `insert link` and
  `follow link` appear only while a block is active; a visible dead command
  teaches a false vocabulary. Availability is `block_active` alone — the
  caret is probed when the command runs, never to decide visibility, because
  the probe is async and the list must be decidable at open.
- **`open loops` stays listed at zero loops.** Running it toggles a signal
  whose overlay renders nothing when the list is empty — the same "absence,
  not a zero" the ember already shows.
- **Deliberate exclusions.** Escape is the overlay grammar itself, not a
  command; plain Enter on an empty day is that state's inline affordance
  ("press enter to start one") and phase 4 brings the real create command;
  Ctrl+click is the mouse spelling of `follow link`, which is already listed.
- **Completeness is enforced twice.** A unit test holds the registry against
  the hardcoded chord list — the audit surface a new chord must touch. And
  the dispatch is an exhaustive `match` with no wildcard arm, so a
  `CommandId` added without wiring does not compile. Every later phase that
  adds a keystroke adds its palette entry in the same change.

## Rejected

- **Chords only** — the exit criterion asks no more, but a palette that
  knows the chords and not the ember makes the mouse-only commands the only
  unnameable ones, which is backwards.
- **Silent no-op rows for contextual commands** — the guard idiom of the
  chords themselves, but a chord you press blind and a row you can read are
  different promises; a listed command that does nothing reads as broken.
- **Deriving the registry from a central keymap** — there is no keymap;
  chords live where their state lives (`adr/2026-08-ctrl-l-link-picker.md`).
  Building one for nine commands is speculative structure.
