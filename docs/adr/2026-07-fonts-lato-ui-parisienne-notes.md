# Fonts: Lato for the UI, Parisienne for note prose

## Context

The design language phase shipped with system font stacks (`ui-sans-serif`,
`ui-monospace`) in `theme.css` and Libertinus Serif in `template.typ`.
Trying real fonts (Cormorant Garamond first) surfaced that the rendered
pane ignores CSS entirely — its font is whatever the Typst template names,
and the embedded compiler only saw `typst_kit`'s embedded fonts.

## Decision

- All UI chrome (labels, buttons, lists, errors) uses **Lato**; the source
  pane and the rendered note use **Parisienne** — the note is visually a
  different thing from the app around it.
- `render.rs` extends the font store with `typst_kit::fonts::system()`
  (feature `scan-fonts`), so templates can name any installed family. This
  keeps the standalone-compilation invariant: the vanilla `typst` CLI scans
  system fonts by default, so a note that compiles in-app compiles at the
  CLI too.
- Fonts are user-installed (`~/.local/share/fonts`), not bundled — single
  user, single machine; bundling can come with a real need.

## Rejected

- Bundling the fonts as assets / embedding in the binary: speculative for a
  single-user app on one machine.
- Keeping the compiler on embedded fonts only and picking a font Typst
  embeds: would force the note font choice to Typst's embedded set.
