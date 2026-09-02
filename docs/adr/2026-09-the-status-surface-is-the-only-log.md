# The status surface is the only log: no log file, no log crate

## Context

`Cargo.toml` carries no `log`, `tracing` or `env_logger`; `src/` holds no
`eprintln!`. Every failure the windowed app can have — a save, a watcher
batch, a delete, an index read, a template seed, a watcher that would not
start — becomes a `status::Notice` with a source and a severity
(`adr/2026-08-status-surface-owns-notices.md`), and `main` carries the two
pre-launch failures in as `SeedTrouble` and the feed's `trouble` because "a
desktop app's stderr is nowhere". The one process that does write to stderr
is the headless `--capture` run (`capture::run`), which has no window to
show a notice in. The 2026-09-02 audit asked where errors are logged; the
answer was decided piecemeal and never written down.

## Decision

**There is no log file and no logging crate.** The status surface is the
windowed app's log: a notice says what happened and to what, stays until
acknowledged or resolved, and its severity ladder is the only routing. A
file under `~/.local/state` that nobody opens is a second status surface
with no reader; the first one is on screen.

**stderr belongs to the headless process.** `--capture` reports its one
failure there because losing a paste silently is the outcome worth
preventing, and a shell is its only screen. The e2e harness redirects the
windowed binary's stdout and stderr to `app.log`, which is why a persona
reads `LOG` when a session refuses to start — that is the harness's file,
not the app's.

## Known ceiling

An acknowledged notice is gone, and a crash leaves nothing behind. If a
failure ever needs a post-mortem, the upgrade is a bounded ring of past
notices in `status`, shown by a palette row — still on screen, still not a
file.

## Alternatives rejected

- **`tracing` to a file** — a dependency, a path, a rotation policy and a
  reader that does not exist, for messages the status surface already
  shows to the one person who can act on them.
- **Mirroring notices to stderr as well** — the desktop launcher discards
  it; under the e2e harness it would duplicate what the scenarios already
  assert through the files.
