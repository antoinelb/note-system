# The watcher feeds the screen

## Context

The file watcher has existed since phase 3 (`adr/2026-07-incremental-vault-watching.md`)
and has never been wired into the app: the index was built once at launch and
never touched again, which `adr/2026-08-links-footer-both-directions.md` had
to work around by reading outgoing links from the live buffer instead of the
index.

Phase 10 forces the issue. Global capture is a **second process** writing a
file into the vault (`adr/2026-08-capture-headless-second-process.md`), and
that design is only viable if the running app notices. The ember counting
unsummarized captures would otherwise be a number that goes stale the moment
you capture something.

## Decision

- **`main` starts the watcher and hands the receiving end to the app** as a
  `VaultFeed` root context — the `Closer` / `CaretProbe` channel. The
  watcher's own `std::sync::mpsc::Receiver` is drained by a plain thread that
  forwards each batch into a `tokio::sync::mpsc` channel, because the shell
  awaits and the watcher's channel blocks. That thread owns the watcher:
  dropping it would stop the debouncer.
- **The shell takes the receiver once**, in a `use_hook`, and spawns a task
  that applies each batch to the index and re-runs the survey. A receiver has
  one owner, so a cell that is already empty starts nothing.
- **Per-batch `Index::open`**, matching every other read in the module. The
  batches arrive debounced and seldom; holding a connection across renders
  would be the exception, not the rule, in this codebase.
- **Two signals are refreshed: the rail's time notes and the open loops.**
  Everything else the screen derives — the "captured today" block, the links
  footer, the month grid's existence marks — is recomputed per render and so
  follows for free.
- **A failure is a notice, never a crash and never silence.** Open, apply
  and re-read all report through one message ("watching the vault: …"), so
  an index that breaks under the app says so instead of quietly freezing the
  screen in the past.
- **A watcher that will not start is a degradation, not a fatal error**: it
  reports on stderr and the app runs on the index it loaded at launch —
  exactly the behaviour of every version before this one.
- **The app's own writes round-trip through the watcher** (autosave, note
  creation, in-app capture). `update_note` is idempotent, so re-reading a
  file the app just wrote costs one parse and changes nothing.

## Rejected

- **Polling the index on a timer** — simpler to wire, wrong in both
  directions: idle CPU when nothing changes, and a visible lag when
  something does. The watcher already exists and is already tested.
- **Keeping one `Index` connection in a signal** — fewer opens, but it makes
  the index a piece of UI state with a lifetime, and every existing read in
  the module opens its own.
- **Refreshing the whole `Loaded` tuple, editor included** — the open buffer
  must not be replaced under the caret because a file changed on disk. Only
  what the screen *derives* from the index is refreshed.
- **Making the watcher mandatory** — a vault on a filesystem without inotify
  would then fail to launch rather than merely fail to notice.
