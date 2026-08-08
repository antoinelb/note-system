# Light table colours derived by one rule: hue kept, value inverted

## Context

The wireframes leave most light-mode table colours undrawn — the "—" cells in the palette table (card fill, capture fill, star field, generated border) — and draw only six of the eight type bars, in dark mode only (`design/wireframes-v0.md` § Palette).
The repo rule is that both themes are filled in together from the first styled rule (`adr/2026-07-design-language-own-phase.md`), so v1 phase 2 had to close the gap in the same pass that mounts the table.
The deck's only light-mode hint is turn 4c: "pale lavender-grey, same constellation lines in silverpoint".

## Decision

**One derivation rule, applied uniformly: keep each dark colour's hue, invert its value against the light ground `#f5f4f9`; the 3px type bars additionally gain chroma (saturation ×~1.4, lightness settled near 47%) because a thin line needs more of it on light than on dark.**

Two dark hues were also missing — the wireframes draw six bars, the full eight-type reference lives in turn 1.
**Organisation and personal get the same mute the six drawn bars are of their turn-1 references** (hue kept, saturation ~halved to ~25%, lightness ~50%; the ratio person `#c98a72` → `#a0765f` sets): organisation `#c9a86a` → `#9f8960`, personal `#a888c9` → `#896ea6`.

The derived values, all in `assets/theme.css`:

| Variable | Dark | Light |
|---|---|---|
| `--card-fill` | `#151226` | `#eae9f2` |
| `--card-capture-fill` | `rgba(16,14,30,.8)` | `rgba(241,239,247,.8)` |
| `--card-generated-edge` | `#443d6b` | `#aaa4cb` |
| `--type-person` | `#a0765f` | `#a36b4d` |
| `--type-organisation` | `#9f8960` | `#a3854d` |
| `--type-source` | `#83875e` | `#8e9550` |
| `--type-concept` | `#7d78a0` | `#5f5699` |
| `--type-claim` | `#6a94a0` | `#4e8797` |
| `--type-idea` | `#6a76a8` | `#4d5ea3` |
| `--type-personal` | `#896ea6` | `#76549c` |
| `--type-project` | `#9a7d95` | `#97598c` |
| `--bar-untyped` | `#4a4566` | `#a5a1b8` |
| `--bar-generated` | `#6f6a8c` | `#8b87a0` |
| `--star-bright` | `#3d3660` | `#c8c4de` |
| `--star-dim` | `#332c52` | `#d4d1e6` |

`--bar-untyped` and `--bar-generated` reuse the values of the faint and muted inks in both themes, but under their own names: they mean "visible debt" and "disposable", not "fourth ink level", and renaming one must not silently restyle the other.

## Alternatives rejected

- **Turn-1 reference hues as the light bars directly** — turn 1 was drawn on light, but the wireframes already re-hued concept (green `#6fb08c` → lavender `#7d78a0`) when muting into dark; deriving light from the dark bars keeps every type's hue identical across the theme toggle, which matters more than fidelity to a reference the deck itself abandoned.
- **Pick by eye, freeze later** — the phase-3 knobs (`sheetW`, `dimOpacity`) earn that treatment because they are single scalars; fifteen colour pairs picked ad hoc would drift out of family, and a rule can be re-run when a hue is ever retuned.
