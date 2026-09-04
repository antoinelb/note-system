# The theme colours follow the Obsidian vault

## Context

The app replaces Obsidian for daily notes, and the user still reads the old vault at
`~/documents/notes-v1` beside it.
The two looked nothing alike: the app shipped the "deep field" palette of the design phase (`adr/2026-08-shipped-ui-is-the-spec.md`), a near-black `#0d0b18` ground under lavender ink, while the vault is a violet-grey Rosé Pine.
Reading the same note in both was a costume change.

The vault's appearance is not one file.
`~/documents/notes-v1/.obsidian/appearance.json` names theme `obsidian` (dark) and `cssTheme` `Border`, with `accentColor` empty; the two enabled snippets, `snippets/style.css` and `snippets/dashboard.css`, set spacing; their only two colour declarations strip a tag's pill (`.cm-hashtag { background: none }`) and put a white mat behind an embedded image, and neither touches a palette variable.
But `.obsidian/plugins/obsidian-style-settings/data.json` does, and heavily: it turns on `accent-color-override-dark`, sets the dark accent to `#c4a7e7`, and replaces six of Border's computed dark tones with literals.
Those six literals are Rosé Pine Moon — base `#232136`, surface `#2a273f`, overlay `#393552`, muted `#6e6a86`, subtle `#908caa`, text `#e0def4` — and `#c4a7e7` is that palette's iris.
So the honest answer to "what colours is the vault" is: Border's structure, Rosé Pine Moon's values.

## Decision

`assets/theme.css`'s two token blocks are filled from the vault, resolved to plain hex and rgba literals.
No calc chain, no runtime accent: the app reads a stylesheet, not the vault's configuration.

**Where each dark value comes from.** `theme-dark-style-select` in `data.json` is `theme-dark-background-default`, which is also Border's own default (`themes/Border/theme.css:1300`), so the plain `.theme-dark` block at `theme.css:3787` applies and none of the `-darker`/`-brighter`/`-black` variants do.
The accent resolves through `.theme-dark.accent-color-override-dark` (`theme.css:3675`), which promotes `--accent-dark-h/-s/-l`; those are emitted by Style Settings from the `accent-dark` picker, declared `format: hsl-split` at `theme.css:1324`, so `#c4a7e7` becomes `hsl(267.19, 57.14%, 78.04%)`.

| app token | vault source | resolution |
| --- | --- | --- |
| `--bg` `#2a273f` | `--background-primary`, Style Settings literal | the surface the user writes on |
| `--card-fill`, `--sheet-fill` `#232136` | `--background-secondary`, Style Settings literal | raised surfaces sit under the ground, the polarity the light theme already used |
| `--card-capture-fill` `rgba(42, 39, 63, 0.8)` | `--background-primary` at 80% | a capture card lets the void through, as it did before |
| `--ink`, `--ink-bright` `#e0def4` | `--text-normal`, Style Settings literal | see below |
| `--ink-muted` `#908caa` | `--text-muted`, Style Settings literal | |
| `--ink-faint` `#6e6a86` | `--text-faint`, Style Settings literal | |
| `--ink-faintest` `#555555` | `--color-base-40` (`theme.css:3818`) | one step below `--text-faint` |
| `--hairline` `#45405a` | `--background-modifier-border` (`theme.css:3841`) | `hsla(267.19, 22.86%, 70.24%, 0.2)` flattened on `--bg` |
| `--border` `#4c4660` | `--background-modifier-border-hover` | the same colour at 0.25 |
| `--hairline-strong` `#524c67` | `--background-modifier-border-focus` | the same colour at 0.3 |
| `--edge`, `--star-bright` `#544a5c` | `--background-secondary-alt` (`theme.css:3847`) | `hsl(275.19, 10.58%, 32.52%)` |
| `--star-dim` `#45405a` | `--background-modifier-border` | the dim star is the hairline tone |
| `--selection` `#c4a7e7` | `--color-accent` | the raised card's border, the tether, the edge node dots |
| `--select-fill` `rgba(196, 167, 231, 0.25)` | `--text-selection` (`theme.css:3864`) | `hsla(--interactive-accent-hsl, 0.25)`, and Border leaves `--interactive-accent` to Obsidian's default `var(--color-accent)` |
| `--sheet-glow` `rgba(196, 167, 231, 0.18)` | `--color-accent` | the glow keeps its own alpha, takes the accent's hue |
| `--markup-link` `#c4a7e7` | `--color-accent` | Border sets neither `--link-color` nor `--text-accent`, so Obsidian's defaults chain both to the accent |
| `--markup-checkbox-done` `#87d37c` | `--checkbox-color` → `--color-green` (`theme.css:3797`, `7373`) | `rgb(135, 211, 124)` |
| `--markup-checkbox` `var(--ink-faint)` | `--checkbox-border-color: var(--text-faint)` (`theme.css:7375`) | |
| `--bar-untyped`, `--card-generated-edge` `#6e6a86` | `--text-faint` | they aliased `--ink-faint`'s value before and still do |
| `--bar-generated` `#908caa` | `--text-muted` | likewise `--ink-muted` |

**Where each light value comes from.** `data.json` carries no light override (its one `Appearance-light@@…` key is suffixed `@@dark`), and `accent-color-override-light` is off, so Border's `.theme-light` at `theme.css:3681` applies unchanged with its own `--accent-light-h/-s/-l` of `232`, `80%`, `64%`.
That hue is visible elsewhere in the same config: the split-background gradient the vault stores is `rgba(90, 109, 237, 0.1)`, and `hsl(232, 80%, 64%)` is `#5a6ded`.

| app token | vault source | resolution |
| --- | --- | --- |
| `--bg` `#ffffff` | `--background-primary` → `--color-base-00` | |
| `--card-fill`, `--sheet-fill` `#f9f9fa` | `--background-secondary` | `hsl(232, 11.33%, 97.75%)` |
| `--ink`, `--ink-bright` `#1b1c22` | `--text-normal` | `hsl(232, 12%, 12%)` |
| `--ink-muted` `#545664` | `--text-muted` | `hsl(232, 9%, 36%)` |
| `--ink-faint` `#9e9fa9` | `--text-faint` | `hsl(232, 6%, 64%)` |
| `--ink-faintest` `#bdbdbd` | `--color-base-40` | |
| `--hairline` `#e7e8f3`, `--border` `#e1e3f0`, `--hairline-strong` `#dbddec` | `--background-modifier-border` at 0.2 / 0.25 / 0.3 | `hsla(232, 32%, 64%, α)` flattened on white |
| `--edge`, `--star-bright` `#d4d4d4`, `--star-dim` `#e0e0e0` | `--color-base-35`, `--color-base-30` | Border's light ramp is neutral grey |
| `--selection`, `--sheet-glow` `#5a6ded` | `--color-accent` | |

**What stays.** The eight `--type-*` hues, the `--ember`, and `--dim-opacity` are unchanged.
The type hues carry the card bars, an app invention with no Obsidian counterpart (`adr/2026-08-light-table-colours-derived.md`); the ember is the design's one warm element and its alert hue, and the vault has no role that answers to it — its nearest neighbour, `--color-orange`, is a callout tint, not a caret.
`--markup-marker`, `--markup-delim`, `--markup-meta`, `--markup-raw` and `--markup-quote-rule` keep aliasing the ink scale, so they follow the vault by inheritance.

**What the vault could not be followed on.**

- *Headings.* The vault gives every heading level a hue of its own — `h1-color-select: h1-color-designated` and its five siblings resolve to `--color-red`, `--color-orange`, `--color-yellow`, `--color-green`, `--color-blue`, `--color-purple` (`theme.css:6684` onward). The markup model has one `--markup-heading` token, so following the vault would mean painting all six levels in h1's salmon `#ff8a78` — wrong for five of them, and `#fe7968` on the light ground reads 2.58:1, which AIR INP-2 will not take for text. `--markup-heading` stays on `var(--ink-bright)`, which is also what Border's own `h1-color-default` would give.
- *The light link and the light done box.* `hsl(232, 80%, 64%)` on white is 4.33:1 and `rgb(120, 186, 126)` is 2.30:1, under INP-2's 4.5:1 for text and 3:1 for a glyph. Both keep the vault's hue and saturation and drop the lightness to the first step that clears the bar: `#5569ec` (l 63%, 4.55:1) and `#4c9452` (l 44%, 3.71:1, which is where the old palette's done box already sat).

`tests/fixtures/vault/templates/template.typ` is the same change: its `light` and `dark` columns exist to mirror `assets/theme.css` so a CSS-drawn block and its compiled-SVG neighbour agree, and its `hairline` is that file's `--hairline-strong`.
The `paper` column is print, not a screen the vault theme reaches, and keeps the shipped ink.

## Bright ink and plain ink are now one value

Obsidian's ink scale tops out at `--text-normal`; there is nothing above it to borrow for `--ink-bright`.
The light theme has collapsed the pair since it was drawn, and every place the app distinguishes them also changes weight (`.rail-row.selected`, `.picker-row.selected`, `.palette-row.selected`, `.cal-week.selected`), which is the encoding AIR INP-3 asks for anyway.
The one place that loses a signal is `.block-active`, the line under the caret — and the vault turns Obsidian's own active-line highlight off (`minimal-style@@active-line-on: false`), so the caret carries it there too.

## Contrast

Every ink tone gains against its own ground.
Dark, on `#2a273f`: muted 3.82:1 → 4.46:1, faint 2.17:1 → 2.79:1, faintest 1.51:1 → 1.93:1; plain ink 11.52:1 → 10.90:1.
Light, on `#ffffff`: muted 3.16:1 → 7.26:1, faint 2.29:1 → 2.63:1, faintest 1.36:1 → 1.88:1, ink 8.90:1 → 16.99:1.
The link clears 4.5:1 in both themes (6.87:1 dark, 4.55:1 light) and the done box clears 3:1 (7.97:1, 3.71:1).

Two pre-existing shortfalls survive the change and are not introduced by it: `--ink-faint` on the calendar's empty days and `--ink-faintest` on the weekday and season labels are below INP-2's 4.5:1 in both themes, as they were before, and the light `--ember` reads 3.19:1 as warning text (it was 2.92:1).
They belong to the design's tone ladder, not to this decision, and are left where they were rather than fixed under cover of a palette swap.

## Alternatives rejected

- **Import `themes/Border/theme.css` wholesale.** It is 8692 lines of Obsidian's own DOM — `.workspace-leaf`, `.nav-file-title`, `.cm-line` — none of which this app renders, and it would carry a `@import`-shaped dependency on a file in the user's vault that the app must never read. The stylesheet stays the app's own; only the numbers came across.
- **Derive from an accent hue at runtime.** Border's chains (`calc(1.25*var(--accent-l) / 4.5)`) would have to be reimplemented as CSS custom properties, and half of them are dead anyway because Style Settings replaces their outputs with literals. Worse, it would put a dial in the app that no ADR asked for and that the settings overlay would then owe a control (`adr/2026-08-settings-overlay.md` holds two controls on purpose). Resolved literals say exactly what will be painted.
- **Read `.obsidian/` at startup.** The app knows nothing about Obsidian and must not gain a dependency on a second vault's plugin data, which the user may reconfigure or delete at any time. This is a one-time port of values a human read and recorded here.
- **Keep the deep-field ground and take only the accent.** Half a costume change: the ground is most of what the eye sees, and the mismatch that prompted this was the ground.
