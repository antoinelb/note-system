# Notes render in the app's theme through a `sys.inputs` theme input

## Context

Rendered notes came out as white typst pages with the palette's light-column
ink, which the phase-8 block fragments turned into white strips on the dark
field — and the design forbids white outright ("dusk paper, no white
anywhere", `design/wireframes-v0.md` § 4a). Templates cannot consume the
app's CSS variables (`adr/2026-07-theme-attribute-on-app-root.md`), so the
theme needs another channel into the compile.

## Decision

- `template.typ` reads `sys.inputs.at("theme", default: "paper")` and picks
  a palette column: **paper** (white page, light-column ink — what vanilla
  typst produces for `make check-vault` and future exports), **light** and
  **dark** (transparent page via `fill: none`, the matching column's ink /
  muted / hairline / link colours; the dark column mirrors
  `assets/theme.css`).
- The app builds one `Library` per theme with the input pre-set
  (`render.rs::themed_library`) and passes `RenderTheme` through
  `VaultWorld`, `render_svg` and `FragmentCache`; the fragment cache key
  includes the theme, since rendering is no longer a pure function of the
  note's bytes alone. Ctrl+T re-renders fragments through the ordinary
  cache-miss path.
- The textarea border from wireframe 1b is dropped: the design's rule that
  "the one warm thing on screen" is the ember **and the caret** wins, so
  the active block is marked by the ember `caret-color` and its mono
  source, not a frame.

## Rejected

- **A CSS `invert`/`hue-rotate` filter on the SVGs** — restyles note bodies
  (the app must never), wrecks the meta line and link hues, and lies about
  every future colour.
- **Hardcoding the dark column in the template** — light mode is a
  first-class sibling, and `make check-vault`/exports want paper.
- **Two template files per theme** — every note imports one template; the
  input keeps the vault's on-disk contract single.
