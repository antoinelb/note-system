//! What `main` hands the window at launch, short of the calls only the
//! desktop runtime can make. `main.rs` keeps the builder and the closures
//! that touch the window or the webview; every decision those closures
//! delegate — the `--capture` dispatch, the skeleton seeding, the watcher
//! bridge, the probe scripts and the shape of what they answer — lives
//! here, inside the coverage gate
//! (adr/2026-09-main-holds-only-the-launch-builder.md).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{capture, template, ui, watch};

/// `wl-paste | app --capture`: a short-lived headless process that writes
/// the paste into the vault and exits, leaving the running app's watcher
/// to notice (adr/2026-08-capture-headless-second-process.md). Answers
/// the exit code when the first argument asks for a capture, `None` when
/// this run is the window.
pub fn capture_cli(
    first_arg: Option<&str>,
    root: Option<PathBuf>,
    now: &jiff::Zoned,
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Option<i32> {
    if first_arg != Some("--capture") {
        return None;
    }
    match capture::run(root, now, input) {
        Ok(path) => {
            // a stdout nobody reads is not this process's failure: the file
            // is already written, and that was the job
            let _ = writeln!(out, "{}", path.display());
            Some(0)
        }
        Err(message) => {
            let _ = writeln!(err, "{message}");
            Some(1)
        }
    }
}

/// A fresh root gets its skeleton and the embedded default templates
/// before the watcher starts, so the very first launch is usable; the
/// failure rides into the status surface like the watcher's
/// (adr/2026-08-templates-seeded-from-embedded-fixtures.md).
pub fn seed_trouble(root: Option<&Path>) -> Option<String> {
    root.and_then(|root| template::seed(root).err())
        .map(|err| format!("{err:?}"))
}

/// Starts the vault watcher and bridges its blocking channel to the async
/// one the shell awaits (adr/2026-08-watcher-feeds-the-ui.md). A watcher
/// that will not start leaves the app on the index it loaded at launch,
/// which is what it had before this existed, and says so through the
/// feed's trouble — a desktop app's stderr is nowhere
/// (adr/2026-08-status-surface-owns-notices.md).
pub fn watcher_feed(root: Option<&Path>) -> ui::VaultFeed {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    match root.map(watch::VaultWatcher::start) {
        Some(Ok(watcher)) => {
            std::thread::spawn(move || pump(watcher, sender));
            feed(Some(receiver), None)
        }
        Some(Err(error)) => feed(None, Some(error.to_string())),
        None => feed(None, None),
    }
}

/// The spawned thread owns the watcher — dropping it would stop the
/// debouncer — and forwards every batch until the app closes the receiver.
fn pump(
    watcher: watch::VaultWatcher,
    sender: UnboundedSender<Vec<watch::VaultChange>>,
) {
    for batch in watcher.changes.iter() {
        if sender.send(batch).is_err() {
            break;
        }
    }
}

fn feed(
    receiver: Option<UnboundedReceiver<Vec<watch::VaultChange>>>,
    trouble: Option<String>,
) -> ui::VaultFeed {
    ui::VaultFeed {
        changes: std::sync::Arc::new(std::sync::Mutex::new(receiver)),
        trouble,
    }
}

/// Where a mouse press landed, in a coordinate the editor speaks: the hit
/// span's `data-start` plus the UTF-16 offset within its text node
/// (adr/2026-08-caret-on-editor-note-bytes.md). Geometry stays the
/// webview's — the caret itself is app state.
pub const HIT_PROBE: &str = "\
const [x, y] = await dioxus.recv(); \
const at = document.caretPositionFromPoint \
    ? document.caretPositionFromPoint(x, y) \
    : document.caretRangeFromPoint(x, y); \
if (!at) return null; \
const node = at.offsetNode ?? at.startContainer; \
const offset = at.offset ?? at.startOffset; \
const el = node.nodeType === Node.TEXT_NODE \
    ? node.parentElement : node; \
const span = el && el.closest('[data-start]'); \
if (!span || !span.closest('.block-active')) return null; \
return [parseInt(span.dataset.start), offset];";

/// The desktop's own opener, for what a `#link` points at — a PDF under
/// the vault, a URL. The name is a constant rather than a call so the gate
/// can hold `open_with` to a launcher that exists and one that does not
/// without ever starting the real one from a test.
pub const OPENER: &str = "xdg-open";

/// Hands `target` to `opener` and returns at once: the child is reaped on
/// a thread of its own, so a launcher that lingers never holds the UI and
/// one that exits leaves no zombie. A launcher that cannot start is the
/// one failure this can see; whatever it does with the target afterwards
/// is its own (adr/2026-09-link-is-for-resources.md).
pub fn open_with(opener: &str, target: &str) -> Result<(), String> {
    let mut child = std::process::Command::new(opener)
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| format!("{opener}: {err}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// What `HIT_PROBE` answered, or `None` for a press outside the active
/// block and for any shape the script never promised.
pub fn hit(value: &serde_json::Value) -> Option<(usize, usize)> {
    let pair = value.as_array()?;
    let start = pair.first()?.as_u64()?;
    let units = pair.get(1)?.as_u64()?;
    Some((start as usize, units as usize))
}

/// The `[count]j`/`k` walk, run entirely inside the webview
/// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md): `hit` reads
/// the character under a client point and refuses anything outside the
/// active block; `cell` measures the rect of the character a step landed
/// on, so the next step never depends on the app-drawn caret, whose move
/// has not flushed yet. The step is the `.source-line` line-height, one
/// constant for the whole pane — the landed glyph's own rect is shorter
/// than its line box, so stepping by it would under-step and probe back
/// onto the row it just left. The loop is bounded twice — by the count
/// the app already clamped to the note's drawn extent, and by a landing
/// that stops changing, measured from the caret's own row so the note's
/// first and last drawn lines stop the very first step.
pub const LINE_WALK: &str = "\
const [goal, down, count] = await dioxus.recv(); \
const hit = (x, y) => { \
    const at = document.caretPositionFromPoint \
        ? document.caretPositionFromPoint(x, y) \
        : document.caretRangeFromPoint(x, y); \
    if (!at) return null; \
    const node = at.offsetNode ?? at.startContainer; \
    const offset = at.offset ?? at.startOffset; \
    const el = node.nodeType === Node.TEXT_NODE \
        ? node.parentElement : node; \
    const span = el && el.closest('[data-start]'); \
    if (!span || !span.closest('.block-active')) return null; \
    return { start: parseInt(span.dataset.start), node, offset, span }; \
}; \
const cell = (landed) => { \
    const box = landed.span.getBoundingClientRect(); \
    if (landed.node.nodeType !== Node.TEXT_NODE) return box; \
    const length = landed.node.length; \
    const from = Math.min(landed.offset, Math.max(length - 1, 0)); \
    const range = document.createRange(); \
    range.setStart(landed.node, from); \
    range.setEnd(landed.node, Math.min(from + 1, length)); \
    const rect = range.getBoundingClientRect(); \
    return rect.height > 0 ? rect : box; \
}; \
const caretEl = document.querySelector( \
    '.block-active .caret, .block-active .caret-box', \
); \
if (!caretEl) return null; \
const seed = caretEl.getBoundingClientRect(); \
const x = goal ?? seed.left; \
const sourceLine = caretEl.closest('.source-line'); \
const height = (sourceLine \
    ? parseFloat(getComputedStyle(sourceLine).lineHeight) : 0) \
    || seed.height; \
if (!(height > 0)) return null; \
let y = seed.top + (seed.height || height) / 2; \
const origin = hit(x, y); \
let landed = null; \
let taken = 0; \
for (let step = 0; step < count; step += 1) { \
    const next = hit(x, down ? y + height : y - height); \
    if (!next) break; \
    const before = landed ?? origin; \
    if (before && next.start === before.start \
        && next.offset === before.offset) break; \
    const rect = cell(next); \
    y = rect.top + rect.height / 2; \
    landed = next; \
    taken += 1; \
} \
if (!landed) return null; \
return [landed.start, landed.offset, x, taken];";

/// Where a freshly mounted caret puts itself in the pane, said in the one
/// vocabulary the DOM answers to. Dioxus's own `scroll_to_with_options`
/// cannot say it: `dioxus-desktop` serialises `ScrollToOptions` as
/// `{behavior, vertical, horizontal}` and hands that object straight to
/// `Element.scrollIntoView`, which reads `block` and `inline` — so every
/// scroll it makes takes `block`'s default, `start`, whatever the caller
/// asked for. Every `zz`, every `j` and every caret move landed at the top
/// of the pane, and no test could see it: the headless DOM has no
/// `scrollIntoView` at all (adr/2026-09-the-caret-line-sits-at-the-centre.md).
///
/// The caret is found the way `LINE_WALK` finds it rather than by the
/// mount's own handle, because the script is what has to name it. The
/// alignment arrives as a one-element array, the shape every script here
/// destructures — a bare string would destructure to its first character.
pub const CARET_SCROLL: &str = "const [block] = await dioxus.recv(); const el = document.querySelector(     '.block-active .caret, .block-active .caret-box', ); if (!el) return false; el.scrollIntoView({ behavior: 'instant', block, inline: 'nearest' }); return true;";

/// What keeps the sink the document's focused element for the life of the
/// window: a click on anything that is not focusable moves the focus to
/// `<body>`, where no handler reads keys, and a refocus asked from Rust
/// would land a round trip later — every key typed in between lost. This
/// listener refocuses the sink in the same event turn, before the next
/// key can be dispatched (adr/2026-09-the-sink-is-the-one-keyboard-socket.md).
/// The window losing focus fires the same event with the sink still the
/// document's focused element, so the refocus is a no-op there.
pub const KEEP_FOCUS: &str = "document.addEventListener('focusout', (event) => { const sink = document.querySelector('.ime-sink'); if (sink && event.target === sink) { queueMicrotask(() => sink.focus({ preventScroll: true })); } }); return true;";

/// What `LINE_WALK` answered: one landing for the whole run, or `None`
/// when it could not take even one step or answered a shape it never
/// promised.
pub fn landing(value: &serde_json::Value) -> Option<ui::Landing> {
    let quad = value.as_array()?;
    Some(ui::Landing {
        start: quad.first()?.as_u64()? as usize,
        units: quad.get(1)?.as_u64()? as usize,
        x: quad.get(2)?.as_f64()?,
        taken: quad.get(3)?.as_u64()? as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir");
        template::seed(dir.path()).expect("the skeleton seeds");
        dir
    }

    #[test]
    fn capture_cli_ignores_every_run_that_is_the_window() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        for first in [None, Some("--help"), Some("capture")] {
            let code = capture_cli(
                first,
                None,
                &jiff::Zoned::now(),
                &mut std::io::empty(),
                &mut out,
                &mut err,
            );
            assert_eq!(code, None, "{first:?} is not a capture run");
        }
        assert!(out.is_empty() && err.is_empty());
    }

    #[test]
    fn capture_cli_writes_the_paste_prints_its_path_and_exits_zero() {
        let dir = vault();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = capture_cli(
            Some("--capture"),
            Some(dir.path().to_path_buf()),
            &jiff::Zoned::now(),
            &mut "pasted from the shell".as_bytes(),
            &mut out,
            &mut err,
        );
        assert_eq!(code, Some(0));
        let printed = String::from_utf8(out).expect("utf-8 path");
        let path = PathBuf::from(printed.trim_end());
        assert!(path.starts_with(dir.path().join("capture")), "{path:?}");
        let written = std::fs::read_to_string(&path).expect("the capture");
        assert!(written.contains("pasted from the shell"));
        assert!(err.is_empty());
    }

    #[test]
    fn capture_cli_reports_a_refusal_on_stderr_and_exits_one() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = capture_cli(
            Some("--capture"),
            None,
            &jiff::Zoned::now(),
            &mut std::io::empty(),
            &mut out,
            &mut err,
        );
        assert_eq!(code, Some(1));
        assert!(out.is_empty());
        assert_eq!(err, b"no vault: define NOTE_VAULT or HOME\n");
    }

    #[test]
    fn seeding_is_silent_on_a_root_and_on_none_and_speaks_on_a_file() {
        let dir = vault();
        assert_eq!(seed_trouble(Some(dir.path())), None);
        assert_eq!(seed_trouble(None), None);
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, "").expect("a plain file");
        let trouble = seed_trouble(Some(&file)).expect("a file is no root");
        assert!(!trouble.is_empty());
    }

    #[test]
    fn a_started_watcher_feeds_its_batches_into_the_async_channel() {
        let dir = vault();
        let feed = watcher_feed(Some(dir.path()));
        assert_eq!(feed.trouble, None);
        let mut receiver = feed
            .changes
            .lock()
            .expect("unpoisoned")
            .take()
            .expect("a started watcher hands over its receiver");
        std::fs::write(dir.path().join("permanent/fresh.typ"), "= fresh\n")
            .expect("a note");
        let batch = receiver.blocking_recv().expect("the sender is alive");
        assert!(!batch.is_empty());
    }

    #[test]
    fn a_watcher_that_cannot_start_rides_its_error_into_the_feed() {
        let feed = watcher_feed(Some(Path::new("/nonexistent/vault/root")));
        assert!(feed.changes.lock().expect("unpoisoned").is_none());
        let trouble = feed.trouble.expect("the start error");
        assert!(!trouble.is_empty());
    }

    #[test]
    fn no_root_means_no_watcher_and_no_trouble() {
        let feed = watcher_feed(None);
        assert!(feed.changes.lock().expect("unpoisoned").is_none());
        assert_eq!(feed.trouble, None);
    }

    #[test]
    fn the_pump_ends_when_the_app_has_closed_the_receiver() {
        let dir = vault();
        let watcher =
            watch::VaultWatcher::start(dir.path()).expect("the watcher");
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        drop(receiver);
        let thread = std::thread::spawn(move || pump(watcher, sender));
        std::fs::write(dir.path().join("permanent/late.typ"), "= late\n")
            .expect("a note");
        // the first batch's failed send is the loop's exit; a pump that
        // ignored it would never return and this join would hang
        thread.join().expect("the pump returned");
    }

    #[test]
    fn an_opener_that_starts_is_ok_and_one_that_cannot_names_itself() {
        assert_eq!(open_with("true", "https://example.org"), Ok(()));
        let refused = open_with("/nonexistent/launcher-for-this-test", "x")
            .expect_err("a launcher that is not there cannot start");
        assert!(refused.starts_with("/nonexistent/launcher-for-this-test: "));
    }

    #[test]
    fn a_hit_is_a_pair_of_integers_and_nothing_else() {
        assert_eq!(hit(&json!([12, 3])), Some((12, 3)));
        assert_eq!(hit(&json!(null)), None);
        assert_eq!(hit(&json!([])), None);
        assert_eq!(hit(&json!([12])), None);
        assert_eq!(hit(&json!(["12", 3])), None);
        assert_eq!(hit(&json!([12, -3])), None);
    }

    #[test]
    fn a_landing_is_a_quad_and_nothing_else() {
        let landed = landing(&json!([40, 2, 17.5, 1])).expect("a landing");
        assert_eq!(
            (landed.start, landed.units, landed.x, landed.taken),
            (40, 2, 17.5, 1)
        );
        assert!(landing(&json!(null)).is_none());
        assert!(landing(&json!([])).is_none());
        assert!(landing(&json!([40])).is_none());
        assert!(landing(&json!([40, 2])).is_none());
        assert!(landing(&json!([40, 2, 17.5])).is_none());
        assert!(landing(&json!([40, 2, 17.5, "1"])).is_none());
        assert!(landing(&json!([40, 2, "x", 1])).is_none());
        assert!(landing(&json!([40, "2", 17.5, 1])).is_none());
        assert!(landing(&json!(["40", 2, 17.5, 1])).is_none());
    }

    #[test]
    fn the_scripts_return_a_value_on_every_path() {
        // the two scripts are text the webview runs, not Rust the gate can
        // see; what a test can hold them to is the contract the parsers
        // above assume — every exit is a `return`
        for script in [HIT_PROBE, LINE_WALK] {
            assert!(script.starts_with("const ["));
            assert!(script.contains("return null"));
            assert!(script.trim_end().ends_with("];"));
        }
        // the third answers whether it found a caret to move, not a value
        // the app parses
        assert!(CARET_SCROLL.starts_with("const ["));
        assert!(CARET_SCROLL.contains("return false"));
        assert!(CARET_SCROLL.trim_end().ends_with("return true;"));
        // the fourth installs a listener and answers nothing the app
        // reads; what it must do is refocus the sink, by class, on focusout
        assert!(KEEP_FOCUS.contains("'focusout'"));
        assert!(KEEP_FOCUS.contains(".ime-sink"));
        assert!(KEEP_FOCUS.contains("sink.focus("));
        assert!(KEEP_FOCUS.trim_end().ends_with("return true;"));
    }
}
