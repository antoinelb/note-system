# A region recompile keeps showing its last good SVG instead of dimmed source

**Superseded by `adr/2026-08-css-draws-the-markup.md`**: the two-region model this ADR's `Side`-keyed shelf exists to paper over is retired — the `Typst` fallback is now one widget per block, cached by that block's own content, so there is no region boundary that moves with the cursor and nothing left for a stale shelf to shelve.

## Context

`adr/2026-08-cursor-split-rendering` merges everything above (and below) the active line into one compiled fragment per side, keyed by `hash_fragment(note, source, theme)` in `FragmentCache`. That key is content-addressed, so it changes on *every* cursor line move — the boundary between "above" and "below" shifts with it. `FragmentCache::probe` had no path back to a previous result once the key changed: a miss always answered `Pending` with nothing to show but the region's raw source, dimmed, while the recompile rode the tier. Two regions recompile on every `j`/`k`, so this dimmed-and-back flash showed on every single line move, including moving back onto a line the app had already rendered a moment before — the exact "jenky" feeling todo 29 named.

`BodyCache` already solved the equivalent problem for the table's card bodies (`adr/2026-08-async-caches-pending-stale.md`): its key is stable (a path), so a miss can look up that same key's last good result and serve it as `stale` while the fresh compile is out. `FragmentCache`'s key carries no such stable identity — there is no "this region" to look up, only "this exact content", which is new on every cursor move by construction.

## Decision

**`FragmentCache` gains a second, small shelf keyed by `Side` (`Above`/`Below`) rather than by content hash — the note has only ever these two regions.** Every time a probe or render call sees a `Ready(Ok(svg))` for its side, win or miss, it refreshes that side's shelf. A miss now answers `Pending { stale, job }`, `stale` pulled from the shelf: almost always `Some` after the note's first paint, because the previous line's compile is sitting there. The UI shows that SVG, undimmed, in place of the raw source, until the fresh compile lands and replaces it.

**The shelf carries an identity, `(note, theme)`, checked before every read.** Its keys are only `Above`/`Below`, so without one, the first probe after switching notes would serve the *previous note's* prose undimmed until the new note's first compile landed, and a theme or font-size step would flash the old theme's pixels the same way (the final review's finding). Same identity keeps the shelf across content-hash misses — the cursor-move case this ADR exists for — and a different one empties it: staleness only ever means "this note under this theme, a moment ago".

The shelf is not swept with `entries`/`touched` — surviving the per-move eviction is the entire point — but it is cleared by `clear()`, alongside everything else: a template touch invalidates every held SVG, fresh or stale, so fragments keep showing nothing but dimmed source across *that* transition, exactly as `adr/2026-08-template-touch-clears-caches.md` already decided (fragments clear outright, unlike bodies, which keep their shelf even there). Only an ordinary content-hash miss — the cursor moving one line — now has a stale answer; an actual invalidation still does not.

## Alternatives rejected

- **Key the shelf by content hash too, keeping the newest N entries as "recently seen"** — recovers nothing: the previous key is precisely the one just evicted, so "keep more of them" only delays the same cliff by a few keystrokes, at the cost of a cache whose size is no longer the fixed two-entry bound `adr/2026-08-cursor-split-rendering` counted on.
- **Diff the two source strings and reuse the compile when they are "close enough"** — there is no cheaper way to know two Typst sources render identically than compiling both; this reintroduces exactly the cost the two-region model was built to avoid.
- **Debounce the recompile so a fast `j`/`k` run only compiles once it settles** — treats the symptom (too many compiles) rather than the one this ADR fixes (nothing to show while any one of them is out), and was explicitly deferred as future performance work by `adr/2026-08-cursor-split-rendering`'s own consequences section.

## Consequences

A region very occasionally shows a slightly stale render for the duration of one compile (tens of milliseconds) — content that was above the cursor a moment ago, still there, one line differently split. This is the same trade `BodyCache` already made and already shipped; nothing about it is new to this app, only to this cache.
