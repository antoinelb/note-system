# gf follows the link under the caret

## Context

Following a link needed Ctrl+Enter, a Ctrl+click, or a palette row
(`adr/2026-08-ctrl-enter-opens-time-links.md`). The user binds `gf` to exactly
this gesture in both of their own PKM setups — the zettelkasten and the wiki
configs each override `gf` buffer-locally to follow a `#zlink` / `#wlink`.

## Decision

`g` then `f` emits `Act::FollowLink`, which the executor answers by calling the
existing `follow_at` callback — the one path Ctrl+Enter, Ctrl+click and the
palette already share. No new link logic: `links::link_at` does the work, and
every kind of link (time note, permanent note, dangling) behaves exactly as it
already did.

`gf` is normal-mode only and refuses behind a verb; `dgf` is not a thing, and
`f` behind a verb is still the find prefix.

## Rejected

- **A second follow implementation for the grammar** — the whole point of the
  `follow_at` seam is that there is one.
- **Replacing Ctrl+Enter** — the chord works from the mouse-driven paths and
  outside normal mode; `gf` is additive, which is what the keyboard-first rule
  asks for (AIR LAY-5: expert accelerators are additive, never exclusive).
