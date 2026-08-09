# Cormorant Garamond replaces Parisienne for note prose

## Context

`adr/2026-07-fonts-lato-ui-parisienne-notes.md` set Lato for the UI and Parisienne for note prose — the rendered pane and the source textarea alike, so entering a block is not a costume change.
Cormorant Garamond had in fact been the first font tried in that decision; daily use has now settled the preference in its favour.

## Decision

- **Note prose — the typst template and the source textarea — uses Cormorant Garamond**; Lato UI chrome and the mono labels stay untouched.
  The two places that named Parisienne change together: `set text(font: …)` in the shared `template.typ`, and the `.block-active` face in `theme.css` (fallback `serif`, no longer `cursive`).
- Sizes are kept as they were (13.5pt rendered, 18px source) — the request was the face, not the scale; the reading-scale ADR stays the authority there.
- Everything else from the fonts ADR stands: system-installed fonts (Cormorant Garamond lives in `~/.local/share/fonts`), the compiler's system scan, no bundling — a note that compiles in-app still compiles at the vanilla CLI.
- The checklist ✓ still names no font: Cormorant Garamond has no check glyph either, and the fallback chain keeps providing it.

## Rejected

- **Changing sizes in the same pass** — a second knob turned without being asked; if Cormorant Garamond reads small at 13.5pt, that is its own decision later.
- **Bundling the font** — still speculative for a single-user app on one machine.
