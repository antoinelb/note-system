# The pre-commit hook is the CI, and `make upgrade` runs the fourth gate

## Context

Four gates exist: `make static`, `make check-vault`, `make test` and
`make e2e`. The hook `make init` installs runs the first three on every
commit; `make e2e` runs when someone remembers, because a scenario costs
seconds and the hook is meant to cost milliseconds
(`adr/2026-08-headless-x11-e2e.md`). No server runs anything. The
2026-09-02 audit asked the question outright: is the hook the CI, or does
a job run all four?

## Decision

**The hook is the CI.** This is a single-user repository on one machine
with one clone; a job on a server would run the same commands later, on a
runner that would have to be taught Xvfb, i3, WebKitGTK and a
software-rendered GL, to report to nobody who is not already sitting at
the keyboard where the hook already ran. `make init` stays the one step a
fresh clone takes, and `core.hooksPath` is how the hook survives a
re-clone.

**`make upgrade` depends on `make e2e`.** The install is the release: it
is the moment a binary starts being used for real notes, and the one
gate the hook skips for speed is the one that drives that binary through
a window. Static, vault and coverage already ran at commit time; the
window runs at install time. A release that fails a scenario does not
install.

## Alternatives rejected

- **A hosted runner (GitHub Actions or similar)** — nothing to gain over
  the hook for one committer, and a second environment to keep in step
  with the one that matters (`WEBKIT_DISABLE_DMABUF_RENDERER`,
  `LIBGL_ALWAYS_SOFTWARE`, an i3 in a temp socket).
- **`make e2e` in the hook** — the hook would take minutes and be skipped
  with `--no-verify` the first time it got in the way, which is worse than
  a hook that is always run.
- **A local `pre-push` hook** — there is no remote to push to; the
  install is the push.
