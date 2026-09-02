# Git backs up the vault from a systemd user timer, and the app stays out of it

## Context

Deleting a note is unconfirmed and trashless
(`adr/2026-07-delete-unconfirmed-no-trash.md`), the in-memory undo
register is ten deep and dies with the process, and every ADR that names
a deeper recovery names git — but the real vault at `~/documents/notes`
had no repository until 2026-09-02. The 2026-09-02 audit ranked this the
first risk in the repo: plain files are the source of truth, and nothing
kept a second copy of them.

## Decision

**The vault is a git repository, committed by a systemd user timer every
five minutes.** Two units under `~/.config/systemd/user/`:

- `notes-commit.service`, oneshot, `WorkingDirectory=%h/documents/notes`,
  runs `git add -A && git diff --cached --quiet || git commit -qm "auto
  <date -Iminutes>"` — a quiet tree commits nothing.
- `notes-commit.timer`, `OnBootSec=2min`, `OnUnitActiveSec=5min`, wanted
  by `timers.target`.

**The vault's `.gitignore` drops `.index/index.db` and keeps
`.index/positions`**: the index is derived and rebuilt on start
(`adr/2026-07-disposable-index-user-version.md`), positions are user data
disguised as index data (CLAUDE.md) and must survive.

**The app knows nothing about git.** No status, no commit command, no
dependency: the timer is a machine setting, not a feature, and the app's
own promise stays "plain files, rebuildable index".

## Alternatives rejected

- **An in-app commit on every save** — a `git2` or shelled-out dependency
  and a new failure mode inside the save path for a benefit the timer
  gives for free.
- **A cron entry** — works, but systemd user timers survive a missed boot
  window (`OnBootSec`) and log to `journalctl --user`, where a failed
  commit is findable.
- **Committing the index too** — a 24 KiB binary rewritten on every
  launch would be most of the history's bytes.
- **A trash directory instead** — rejected once already in
  `adr/2026-07-delete-unconfirmed-no-trash.md`; git is the trash with a
  history.
