# Embedded typst: one `World` per compilation, embedded fonts, packages rejected

## Context

Phase 4 embeds the typst compiler to turn a note into SVG.
The compiler asks the host for everything it needs through the `World` trait (`library`, `book`, `main`, `source`, `file`, `font`, `today`), so this trait is the whole boundary between the app and typst.
`adr/2026-07-import-template-racine.md` already fixed the contract the `World` must honour: the vault is the typst root, and notes import `/templates/template.typ` root-absolute.

Verified against typst 0.15.1: a `World` rooted at `tests/fixtures/vault` compiles `permanent/zettelkasten.typ` through its template import and renders 58 kB of SVG in 41 ms on a debug build, 8 ms on release.

## Decision

- **One `World` per compilation**, constructed from `(vault root, vault-relative path, note text)`. Taking the text as an argument rather than reading it inside `source()` is what lets phase 5 render an unsaved buffer with no change to this layer.
- **Fonts and standard library are process-wide `LazyLock` statics.** Fonts are parsed once, not once per note; `typst_kit::fonts::FontStore` maps directly onto `World::book` and `World::font`.
- **Embedded fonts only** (`typst-kit` feature `embedded-fonts`: Libertinus Serif, New Computer Modern, DejaVu Sans Mono). Rendering is then a pure function of the vault, identical on any machine and assertable in tests. Scanning system fonts would make a font install able to silently change output, including test output.
- **Path resolution goes through `VirtualPath::realize(&root)`** — the sanctioned typst API, which is also the sandbox. Never `root.join(..)` on a path from a note.
- **Packages are rejected explicitly**: a `FileId` whose root is `VirtualRoot::Package` returns a `FileError`, it does not fall through to the filesystem. The vault is self-contained by the same reasoning that rejected local packages for the template.
- **`today()` returns `None`, and this is load-bearing rather than deferred work.** It is the only method on this `World` whose answer is not a function of vault bytes; every other one is a process constant or a file read. `adr/2026-07-cache-svg-par-chemin.md` keys the SVG cache on a hash of the note's text, which is sound only while rendering is a pure function of that text. A live clock breaks it silently: a note rendered before midnight keeps showing yesterday's date indefinitely, because its bytes never changed and so its hash never changed. An explicit "unable to get the current date" is the better failure.

  Accepted cost: a note using `datetime.today()` compiles under the vanilla `typst` CLI but errors in the app — the app is stricter than the CLI. The need is small in practice, since daily notes carry their date in `#meta(created: ..)`, written once at creation, rather than reading a clock.

  Wiring it later is a two-part change, not a one-line one: `today()` from `jiff` **and** the date in the cache key (or clock-using notes exempted from the cache). Neither half is correct alone.

## Alternatives rejected

- **A long-lived `World` with an internal source cache** — typst's docstring asks loading functions to cache, but at note scale a compile is tens of milliseconds and the cache that actually matters is the SVG one. A shared mutable `World` would need `Send + Sync` interior mutability and a staleness story, for no measured gain.
- **Reading the note from disk inside `source()`** — one fewer argument now, but phase 5 needs to compile text that is not on disk yet.
- **Scanning system fonts** — non-deterministic rendering; revisit only when the template needs a font the bundle lacks.
