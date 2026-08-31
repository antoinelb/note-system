# The shipped window is tested by keystrokes in a headless X server

## Context

`make test` proves the components through `dioxus_ssr` (`adr/2026-07-ui-covered-at-100.md`), and proves them thoroughly: 333 tests in `src/ui.rs` fire real `keydown` and `mouse` events into a `VirtualDom` and assert on the markup that comes back, at 100% of lines and regions.

What no test in that file can see is the window.
`dioxus_ssr::render` returns a string; it does not lay anything out, does not compile a Typst fragment to a surface, does not decide what has focus, and cannot tell whether a keystroke pressed on a keyboard ever reaches the webview.
Every defect that lives in *that* gap — a chord swallowed by the wrong pane, an overlay covering the thing it asks about, a caret drawn where the text is not, an SVG that never repaints — is invisible to the whole suite by construction.

`adr/2026-07-ui-covered-at-100.md` already looked at Playwright and rejected it: "tests the real app, but produces no Rust coverage and needs `dx serve` plus a browser in the loop."
That rejection is now stronger than it was written.
The neighbouring repo can run Playwright because its app is a web bundle served over HTTP; this one is `dioxus = { features = ["desktop"] }`, a wry window backed by WebKitGTK.
There is no URL to point a driver at and no CDP endpoint to attach to, and Playwright's WebKit is its own patched build, not a GTK application's webview.
The door is not narrow — it is not there.

The window is nevertheless an ordinary X11 client, and the machine already has everything needed to drive one.

## Decision

**A suite under `tests/e2e/`, run by `make e2e`: `Xvfb` for the display, `i3` to manage it, `xdotool` for the keystrokes, `import` for the screenshots.**
No new dependency, no `node_modules`, nothing to install.

### The vault is the oracle, not the pixels

Every assertion is a bounded poll of a `.typ` file under the run's own vault.
That is not a compromise — it is the strongest oracle this app has.
`adr/2026-07-debounced-autosave.md` promises that "the file on disk is never more than half a second behind the screen", which makes the file a faithful and *stable* witness: it does not move when a font hints differently or a theme token changes, the way a screenshot comparison does.
A scenario types like a person and then reads what the person would have saved.

Screenshots are taken, and are for a human or a persona to look at. They are never asserted on.

### A window manager is mandatory

A bare `Xvfb` has no window manager, so nothing owns `_NET_ACTIVE_WINDOW`; `xdotool windowactivate` refuses ("your windowmanager claims not to support `_NET_ACTIVE_WINDOW`") and every keystroke afterwards is delivered to the root window and silently lost — the suite would go green while touching nothing.
`i3` runs inside the display, with its IPC socket **inside the run's temp directory**: pointed at the default path it collides with the developer's own session and dies with `Address already in use`, and a leak from a killed run poisons the next one.

### Each run owns its world

A `mktemp -d` holding a copy of `tests/fixtures/vault` with `.index/` removed, so the run also proves the index rebuilds; `Xvfb -displayfd`, so the display number is allocated rather than guessed and two runs never collide; and an `EXIT` trap that tears all three processes down, keeping the directory only when something failed.
The binary is the release build: a debug Typst compile turns every settle into a race.

`make e2e` stays **out of the pre-commit hook** (`make static && make check-vault && make test`). A scenario costs seconds; the hook is milliseconds. It also costs nothing in coverage — `cargo llvm-cov` is not passed `--include-tests`, and shell is not instrumented at all.

### The same launch, without the trap, is a persona session

`tests/e2e/session.sh start <name>` runs `e2e_start` and lets the window outlive the shell, so `.claude/agents/note-taker.md` — someone who keeps notes and knows nothing about the code — can explore for fifty keystrokes, look at screenshots, and report what felt wrong.
Naming the session isolates concurrent personas: one display, one vault, one window each.

The persona's report is **not documentation** and is not committed. A finding that survives review becomes either an ADR, because it was a decision, or a scenario under `tests/e2e/`, because it was a regression. This is the lesson the neighbouring repo recorded when three hand-driven personas found real defects and "rien n'en est resté d'exécutable".

## Alternatives rejected

- **Playwright, as next door.** No attachable endpoint exists for a wry window. Reaching one means shipping the app to the web — a renderer port, for a test suite.
- **A `tests/scenarios` target mirroring `2026-08-tests-de-scenario-dans-ui`.** That repo needed one because its orchestration lives in a `#[cfg(target_arch = "wasm32")]` module its coverage gate excludes: a layer no Rust test could reach. Here `src/ui.rs` compiles natively, instantiates `App` with injected contexts and fires real events, at 100%. The hole it patches does not exist in this repo; the hole that does exist is the window, and only a window can fill it.
- **A Rust integration test shelling out to `xdotool`.** It would be collected by `cargo test`, so it would run inside `make test` and under the 100% gate it has no business being measured by, and cargo's harness buys nothing here that `for t in tests/e2e/*.test.sh` does not.
- **Screenshot comparison as the assertion.** One font hint or one theme token repaints every reference image, and a suite that cries at every repaint stops being read.
- **Driving the developer's real display.** The keystrokes would land in whatever window happens to have focus, and a persona would type its exploration into the editor you are reading this in.
- **A fixed `:99` and a fixed sleep.** Both were tried while proving this out. The display collided with a leaked server; the sleeps were either a flake or a tax paid on every run. Every wait here is a bounded poll on the condition itself.
