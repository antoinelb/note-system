# `main.rs` holds only the launch builder, and the coverage gate no longer excuses the file

## Context

`adr/2026-07-ui-covered-at-100.md` promised that `main.rs` "keeps only
the launch call — still zero logic", and `makefile` excused the whole
file from `cargo llvm-cov` on that promise. By 2026-09-02 the file was 247
lines: the `--capture` dispatch with its exit codes, the watcher thread
bridging its blocking channel to the shell's async one, the skeleton
seeding, two webview scripts (`LINE_WALK` alone is sixty lines) and the
parsers of what those scripts answer. The 2026-09-02 audit named it the
first harness gap: `j`/`k` across a wrapped line, a paste over a closed
stdout, a watcher that refuses to start — all outside the 100% the gate
reports.

## Decision

**A `launch` module holds every decision `main` used to make**, and is
covered like any other module: `capture_cli` (the dispatch, taking its
argument, root, clock and the three streams as parameters), `seed_trouble`,
`watcher_feed` with its `pump` loop split out so the "app closed the
receiver" exit is a unit test and not a race, the two scripts as constants,
and `hit`/`landing` parsing the JSON the scripts answer.

**`main.rs` is the builder and the closures that call the desktop
runtime**, nothing else. Those closures — `window().close()`,
`document::eval` — cannot run outside a wry window, so `fn main` carries
`#[cfg_attr(coverage_nightly, coverage(off))]` and the makefile's
`--ignore-filename-regex` shrinks to `lib.rs` and `mod.rs`. The difference
from the old exclusion is what happens to the next helper someone adds to
`main.rs`: it is counted, the gate fails, and the helper moves into
`launch`. The exemption is now one function wide instead of one file wide.

`serde_json` becomes a direct dependency for the two parsers' signatures;
it was already in the tree under dioxus.

## Alternatives rejected

- **Keeping the file-wide exclusion** — the exemption had grown to 247
  lines without anyone deciding it should, which is exactly how a total
  number stops being total.
- **Moving the closures into `launch` behind traits** — an abstraction
  whose only second implementation would be the test's, and the closures
  are one line each around the runtime call.
- **A `#[cfg(test)]` stub of the desktop runtime** — mocks the one thing
  `make e2e` exists to exercise for real (`adr/2026-08-headless-x11-e2e.md`).
