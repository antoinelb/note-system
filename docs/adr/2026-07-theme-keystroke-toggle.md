# Theme chosen by a keystroke toggle, dark by default

## Context

Phase 6 ships both themes as CSS custom properties (`2026-07-design-language-own-phase.md`), which left open how the active theme is picked: OS `prefers-color-scheme`, an explicit toggle, or both.

## Decision

A keystroke — **Ctrl+T** — toggles dark ⇄ light.
Dark is the default on every launch; the choice lives in a session signal and is not persisted.
The OS preference is not consulted.

## Alternatives rejected

- **Follow `prefers-color-scheme`** — the design names dark as *the* main mode (`design/wireframes-v0.md`), not "whatever the OS says"; and a media query evaluated by the webview lives outside the app's one source of truth (the `data-theme` attribute), unobservable in the headless test harness.
- **A visible toggle control** — the chrome is frozen at one line and one number (`design/wireframes-v0.md` § Chrome); everything else is a keystroke.
- **Persisting the choice** — a settings file for one bit is machinery nothing else needs yet; dark-on-launch *is* the design's stance, and light mode stays one keystroke away.
