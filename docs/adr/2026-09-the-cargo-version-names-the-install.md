# The Cargo version names the install; `0.N.0` is roadmap version vN in daily use

## Context

`Cargo.toml` said `0.1.0` from the initial commit (2026-07-23) and was bumped
once, to `0.2.0`, on 2026-08-24 with the tag `v0.2.0` — 103 commits later, in
the middle of v2, matching no roadmap boundary: v0 closed on 2026-08-07, v1
on 2026-08-09, v2 on 2026-08-31. The roadmap's v0–v3 and Cargo's `0.x.y`
share a letter and nothing else, and the 2026-09-02 audit asked what one
means against the other.

## Decision

**The Cargo version names an installed binary, and the minor is the highest
roadmap version in daily use.** `make upgrade` is the release
(`adr/2026-09-the-pre-commit-hook-is-the-ci.md`); the version is what that
release is called. `0.1.0` covered the v0 and v1 installs, `0.2.0` is v2's
vim layer in daily use, `0.3.0` will be the first install that carries v3's
AI, and `1.0.0` the install after the roadmap is spent. The patch counts
installs worth naming between two roadmap versions — none so far — and a
`vX.Y.Z` tag marks each bump. Nothing reads the version at runtime, so the
number costs nothing to keep honest and nothing when stale.

## Alternatives rejected

- **Bumping at every `make upgrade`** — a number that changes weekly names
  nothing; the install log is `git log`.
- **Minor per closed roadmap version, retroactively** — the tree would owe
  a `0.3.0` for v2 and `0.2.0` would have meant v1, which it never did;
  rewriting the one tag that exists buys no clarity.
- **`1.0.0` now, because it is a daily driver** — the roadmap still holds an
  unstarted version; `1.0` is the word for "the plan is spent".
