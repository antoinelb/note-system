# The theme attribute lives on the rendered app root, not `<html>`

## Context

`2026-07-design-language-own-phase.md` fixed the selector shape as `:root` (dark) and `:root[data-theme="light"]`, assuming the attribute would sit on `<html>`.
Dioxus rsx cannot render attributes on `documentElement`; only `document::eval` reaches it, and eval is invisible to the `VirtualDom` + `dioxus_ssr` harness the UI is covered by (`2026-07-ui-covered-at-100.md`).

## Decision

The attribute sits on the app's root element: a rendered `div.app[data-theme]` wrapping every screen, including the vault-error one.
`theme.css` selectors are `.app { dark }` and `.app[data-theme="light"] { light }` — custom properties inherit to every descendant, which is all resolution needs.
The attribute is rendered in both states (`"dark"`/`"light"`, never absent), so the toggle is ordinary signal state and the theme test is a plain string assertion on the SSR output.

Scope of the no-literal rule, made explicit here: it governs the app — `src/` and every stylesheet — and the exit-criterion grep runs over `src/`.
Typst templates are rendered *content*, outside the CSS variable mechanism; their colour literals must still be values from the design's palette table, because they cannot consume CSS variables and `make check-vault` compiles them with the vanilla typst CLI.

## Alternatives rejected

- **`document::eval` setting `documentElement.dataset.theme`** — untestable headlessly, and splits the theme across a signal and a DOM side effect.
- **Duplicating the variables on both `:root` and the wrapper** — two sources of truth.
