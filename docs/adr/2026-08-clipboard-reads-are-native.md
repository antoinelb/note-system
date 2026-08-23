# Clipboard reads are native and serialized

## Context

The editor's Ctrl+V, vim `p` / `P`, and in-app capture all read through the injected `Clipboard` seam.
Its production adapter used `navigator.clipboard.readText()`, but Wry disables JavaScript clipboard access by default on Linux and Dioxus 0.7 exposes no `Config` switch for Wry's `with_clipboard(true)`.
The adapter erased that rejection with `.await.ok()`, so every paste became a silent no-op while the headless seam tests stayed green.
This replaces the read-side mechanism recorded by `2026-08-capture-headless-second-process.md` and `2026-08-hidden-ime-sink.md`; their behavior decisions still stand.

## Decision

Taken with the user (2026-08-23):

- **A native `clipboard::Reader` owns reads** through arboard's text-only X11 and Wayland backends.
- **One worker serializes every read off the UI thread** through a bounded eight-request FIFO.
  It keeps one native clipboard instance while healthy, drops it after an operation failure, and retries initialization on the next request.
- **The `Clipboard` seam returns `Result<String, String>`** instead of collapsing failure into absence.
  The status surface reports `Source::Clipboard`; a failed read changes no note and creates no capture, while a later successful read resolves the warning.
- **Clipboard writes stay on the existing `ClipboardWrite` seam.**
  Copy works in the running app, and replacing its adapter is outside this paste failure.

## Rejected

- **Enable Wry's webview clipboard flag** — the needed builder exists below Dioxus, but Dioxus 0.7 does not expose it through desktop `Config`.
- **Shell out to `xclip` or `wl-paste`** — that makes ordinary editing depend on session-specific executables and a process launch per paste.
- **Keep the JavaScript read and surface its error** — it would explain the refusal without restoring paste on the affected Linux webview.
- **Replace reads and writes together** — broader than the observed paste-only failure and not surgical.
