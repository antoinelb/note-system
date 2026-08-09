# A new card lands at the viewport centre

## Context

Phase 4 creates cards before phase 8's auto-placement exists; the roadmap asks where they land until then.
The origin fallback grid is deterministic but may be far off-screen from where the user is working.

## Decision

- The new card appears centred in the viewport: `table::spawn_position(viewport, pan)` converts the viewport centre to canvas coordinates, and creation writes that position to the store immediately — the debounced save persists it.
- **Viewport size is injected** as a root-context closure (`ui::Viewport`, the `Closer` idiom): `main` reads the real window's logical inner size, headless tests inject a fixed one, and an absent context falls back to `table::DEFAULT_VIEWPORT` (1280×800) — deterministic, never an error.
- Phase 8 supersedes the store write: the stamp becomes a session-only birth slot so created cards can drift to their links (`adr/2026-08-auto-place-strongest-link-ring.md`).

## Rejected

- **Unplaced (origin grid)** — a note created "here" appearing somewhere else breaks the gesture's promise.
- **Probing the window at call time from `ui`** — `dioxus::desktop::window()` does not exist headless; injection keeps the component testable, the established pattern.
