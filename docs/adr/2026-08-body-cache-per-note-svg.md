# Body zoom gets its own per-note SVG cache

## Context

Body zoom renders whole notes as SVG.
The existing `FragmentCache` is per-block and swept at every activate/deactivate/note-switch — a policy built for one open note, which would evict every card body on the first sheet interaction.

## Decision

- **A second cache, `BodyCache`** (`render.rs`): whole-note SVGs keyed by `(vault-relative path, theme)`.
  It reads the file itself on a miss and caches errors like `FragmentCache` does — an unreadable or uncompilable note is an error entry, never a panic.
- **Invalidation is the watcher's**: every `Touched`/`Removed` path drops its entries (both themes — the file changed for both alike); a `Rescan` clears the cache.
  In-app edits flow through the same watcher via the autosave's disk write, so there is no second invalidation path.
- Compilation happens synchronously on the render path (the fragment panes' precedent), bounded by culling to the visible cards and by the cache to once per note per theme.
  If the manual jank check over a dense cluster fails, the escalation is spawned compiles with a placeholder — room left, not built.
- Like the fragment cache: in-process only, held as a plain `use_hook` Rc<RefCell<…>> — a memo store, not UI state; nothing persists to `.index/`.

## Rejected

- **Reusing `FragmentCache` with whole-file sources** — its sweep policy is the opposite lifecycle; entangling them would make the sheet evict the table.
- **Persisting SVGs to `.index/`** — hashes and rendered output on disk for a cache the watcher can rebuild in memory; the index stays derived-and-disposable.
- **Async compiles now** — machinery for a jank not yet observed; the cache and culling bound the work first.
