# SVG cache keyed by path, validated by content hash

## Context

Phase 4 caches rendered SVG so that re-opening a note does not recompile it.
A compile costs ~40 ms debug / ~8 ms release, so the cache is a comfort, not a necessity — which sets the bar for how much machinery it deserves.

## Decision

`HashMap<PathBuf, (u64, String)>`: one entry per note, holding the hash of the source it was rendered from and the resulting SVG. A lookup recomputes the hash of the current text and re-renders on mismatch.

Keying by *path* and storing the hash as a validity stamp keeps the cache bounded by vault size. Keying by content hash would leave one dead entry behind per edit, growing without bound over an editing session.

The cache lives in a plain struct in `render`, not in a Dioxus signal: it is not reactive state, and writing to a signal during render is a re-render hazard.

**Known ceiling:** the hash covers the note's own text, not `templates/template.typ`. Editing the shared template while the app runs leaves stale SVGs until restart. Acceptable while the app is read-only; phase 5 wires the phase-3 watcher to clear the cache when anything under `templates/` changes. This is recorded in a comment at the cache definition.

## Alternatives rejected

- **Hash note text + template text** — correct, but every cache *lookup* would have to read the template from disk, which is most of the work the cache exists to avoid.
- **No cache at all** — defensible at 8 ms, but the roadmap asks for it and it is ~15 lines; the invalidation logic is also where phase 7's per-block cache will grow from.
- **Cache in a `Signal`** — makes the UI depend on cache internals and turns every render into a potential signal write.
