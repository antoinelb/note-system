# The design language gets its own phase, before the logs screen — with both themes from the first rule

## Context

The design was frozen in `design/wireframes-v0.md` (palette, type scale, chrome, five states) and nothing in the codebase reflects it.
`find . -name '*.css'` returns nothing; `src/ui.rs` emits `class: "note-list"`, `"viewer"`, `"render-error"` against zero rules; `src/main.rs` has no stylesheet.
The roadmap had no task for it anywhere — phase 6 (the logs screen) listed layout tasks only, so the palette would have been invented component-by-component while fighting a three-pane layout.

Two design requirements had no home at all:

- `wireframes-v0.md` § The two screens: *"Dark is the main mode; light mode is a **first-class sibling, not an afterthought**."*
  The palette table carries a full light column and state `6e` is the logs screen in light mode.
  Neither `plan.md` nor the roadmap contained the word "light".
- `wireframes-v0.md` § Typography: the note prose spec (serif body, title scale) is explicitly **not** an app concern — *"the app never restyles them; the wireframes' serif prose stands in for the template's own output."*
  It belongs in `templates/template.typ`, which is still `set text(size: 11pt)` with the default font.

## Decision

A new **phase 6 — the design language**, between writing (5) and the logs screen (7); the old phases 6–9 become 7–10.
It ships no screen: it turns the frozen design into `assets/theme.css` plus the one-line chrome, and gives `template.typ` the design's prose typography.

**Both themes exist from the first styled rule.**
Every colour goes through a custom property — `:root` for dark, `:root[data-theme="light"]` for light — and no colour literal ever appears outside `theme.css`.
*(Amended by `2026-07-theme-attribute-on-app-root.md`: the attribute lives on the rendered `.app` root, not `<html>`, so the selectors are `.app` / `.app[data-theme="light"]`.)*
That constraint is the whole point: it is nearly free while the rules are being written and expensive as a retrofit, and the design has already picked all ~20 light values, so there is nothing left to eyeball.

Delivery is Dioxus' own mechanism, `document::Stylesheet { href: asset!("/assets/theme.css") }` (`.claude/dioxus.md` § Styles) — not inline `style:` attributes, which cannot express `:root` variables or a theme switch.

How the theme is *chosen* (OS `prefers-color-scheme`, an explicit toggle, or both) stays open as a phase-6 ADR; it does not affect the variable structure.

## Alternatives rejected

- **Fold the styling tasks into the logs-screen phase** — no renumbering, one bigger phase, but styling decisions then get made under layout pressure, component by component. That is exactly how a frozen palette drifts, and the logs screen is three panes' worth of opportunity to drift.
- **Defer all styling until v0 is functionally complete** — fastest to a daily driver, but the app being daily-driven would look nothing like the thing that was designed, and the visual pass lands as one large diff against a 100 %-coverage rule.
- **Dark only in v0, light in v1** — smallest phase 6, but it leaves half of a frozen design unimplemented for a version and rewrites every rule when light lands. The design's own wording ("not an afterthought") is a direct instruction against it.
- **Tokens now, light values later** — keeps the architecture right at half the eyeballing, but the eyeballing was already done by the design; deferring buys nothing and risks the light column bit-rotting against the dark one.
