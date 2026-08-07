# Global capture is a headless second process, not IPC

## Context

`plan.md` § Capture notes wants captures "created instantly via global
hotkey or paste", and phase 10 asks explicitly whether that is a true global
hotkey or "a DE shortcut launching `app --capture` through single-instance
IPC".

Two facts decide it. Wayland does not let an application grab a global
hotkey — the compositor owns them, so *something* outside the app has to
bind the key whatever we build. And the vault is plain files with a watcher
already running over them: a second process writing a `.typ` file is already
a supported way to change the vault, tested since phase 3.

## Decision

- **A DE shortcut runs `app --capture`.** It reads the paste on **stdin**
  (`wl-paste | app --capture`), writes one file into `capture/` from the
  capture template, prints its path and exits. No window, no IPC, no
  single-instance lock.
- **The running app finds out through the watcher**, like any other external
  change (`adr/2026-07-incremental-vault-watching.md`). This is what makes
  the second process viable at all, and it is why the watcher had to reach
  the UI in the same phase (`adr/2026-08-watcher-feeds-the-ui.md`).
- **It works with the app closed**, which the IPC design could not do
  without launching the whole desktop app to write nine lines of text.
- **Stdin, not a `--text` argument**: pastes contain newlines, quotes and
  whatever else was on the clipboard, and a pipe carries them without a
  shell quoting them. Bounded at **1 MiB** — a capture is a paste, not a
  file transfer — and non-UTF-8 input is refused rather than lossily
  converted. Both are errors on stderr with exit code 1: this process has no
  window to show a notice in, and silently losing a paste is the one outcome
  worth engineering against.
- **The logic lives in `capture::run`**, which takes the vault, the clock
  and the reader as parameters. `main` is the only untestable part (it is
  excluded from coverage), and it does nothing but read `--capture`, the
  environment and stdin.
- **In-app, the same thing is a chord: Ctrl+Shift+V.** It reads the
  clipboard through an injected JS `navigator.clipboard.readText()` — the
  `CaretProbe` seam — and writes the capture the same way, reporting the id
  as a notice. A webview that refuses the read captures nothing rather than
  an empty note. This is the roadmap's "hotkey + paste" for the case where
  the app is already in front of you and no DE shortcut is configured.
- **Nothing opens the capture afterwards.** Captures are fire-and-forget in
  v0: the logs centre pane shows time notes only, so a capture is written,
  counted as an open loop, and summarized later — outside the app until v1's
  table gives it somewhere to live.

## Rejected

- **Single-instance IPC** (socket, forward the paste, app raises a capture
  surface) — a protocol, a lock file, a stale-socket story and a
  focus-stealing question on Wayland, all to end up writing the same file
  the watcher would have picked up anyway. And it cannot capture when the
  app is not running, which is exactly when a quick idea arrives.
- **A true in-app global hotkey** — not available on Wayland without a
  portal, and the portal still amounts to "the desktop binds a key for you".
- **A `--text "..."` argument instead of stdin** — one shell-quoting bug
  away from mangling every paste containing a quote or a newline.
- **Reading the clipboard in the headless process** — it would need a
  clipboard library and a running compositor connection; `wl-paste` (or
  `xclip`, or anything else) already does that, and piping keeps the app
  out of the display server's business.
