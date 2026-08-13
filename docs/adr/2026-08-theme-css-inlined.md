# The theme stylesheet is inlined into the binary

## Context

`assets/theme.css` was pulled in with `document::Stylesheet { href: asset!("/assets/theme.css") }`.
Manganis decides at *runtime* whether the app is bundled: `dioxus-core-types` returns "not bundled" only when `CARGO_MANIFEST_DIR` is set in the environment, which `cargo run` sets and a launched binary does not.
Outside `cargo run`, the asset therefore resolves to `/assets/theme-<hash>.css` inside a `dx` bundle that a plain `cargo build`/`cargo install` never produces, and the app opens unstyled.

This surfaced the first time the app was installed with `cargo install --path .`.

## Decision

Inline the stylesheet: `document::Style { {include_str!("../assets/theme.css")} }`.

The CSS lives in the binary, so the app is styled however it was built and launched — `cargo run`, `cargo install`, a copied binary, a `dx` bundle — and no longer depends on the repository staying at the path baked in at compile time.

`assets/theme.css` stays a separate file, so the "no colour literal outside `assets/theme.css`" invariant is untouched.

## Alternatives rejected

- **Build and run through the `dx` CLI.** Adds a second build tool for one file, and `cargo install` — the way this app is meant to be installed — still would not work.
- **Set `CARGO_MANIFEST_DIR` in a wrapper script.** Lies to a library about its build context, and keeps the installed binary tied to the repository's path on disk.
