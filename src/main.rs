use note_system::{
    capture, clipboard, compute, template, time, ui, vault, watch,
};

fn main() {
    // `wl-paste | app --capture`: a short-lived headless process that writes
    // the paste into the vault and exits, leaving the running app's watcher
    // to notice (adr/2026-08-capture-headless-second-process.md). Checked
    // before the window is built, since this run has no window.
    if std::env::args().nth(1).as_deref() == Some("--capture") {
        match capture::run(
            vault::vault_path(),
            &jiff::Zoned::now(),
            &mut std::io::stdin().lock(),
        ) {
            Ok(path) => println!("{}", path.display()),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        return;
    }

    let root = vault::vault_path();
    let clipboard = clipboard::native();

    // a fresh root gets its skeleton and the embedded default templates
    // before the watcher starts, so the very first launch is usable; the
    // failure rides into the status surface like the watcher's
    // (adr/2026-08-templates-seeded-from-embedded-fixtures.md)
    let seeding = root
        .as_deref()
        .and_then(|root| template::seed(root).err())
        .map(|err| format!("{err:?}"));

    dioxus::LaunchBuilder::new()
        .with_cfg(dioxus::desktop::Config::new().with_menu(None))
        .with_context(ui::VaultRoot(root.clone()))
        .with_context(ui::SeedTrouble(seeding))
        // the watcher's own thread forwards its batches into the async
        // channel the shell awaits (adr/2026-08-watcher-feeds-the-ui.md);
        // a watcher that will not start leaves the app on the index it
        // loaded at launch, which is what it had before this existed
        .with_context(watcher_feed(root.as_deref()))
        // the compute tier: typst compiles and index surveys run on two
        // worker lanes instead of the UI thread; the headless tests omit
        // this and get the inline adapter
        // (adr/2026-08-compute-tier-worker-seam.md)
        .with_context(compute::threaded())
        // called from the keydown handler, where the runtime context that
        // window() reads is current
        .with_context(ui::Closer(std::sync::Arc::new(|| {
            dioxus::desktop::window().close()
        })))
        // read when a note is created, from the overlay's handler — the same
        // window() context note as the Closer
        // (adr/2026-08-new-card-lands-at-viewport-centre.md)
        .with_context(ui::Viewport(std::sync::Arc::new(|| {
            let window = dioxus::desktop::window();
            let size =
                window.inner_size().to_logical::<f64>(window.scale_factor());
            (size.width, size.height)
        })))
        // the app's single clock read
        // (adr/2026-07-today-injected-root-context.md)
        .with_context(ui::Today(time::today()))
        // read again only when a capture is stamped, which is the one thing
        // that needs the time of day (adr/2026-08-capture-timestamp-ids.md)
        .with_context(ui::Now(std::sync::Arc::new(jiff::Zoned::now)))
        // Native reads bypass WebKitGTK's disabled JavaScript clipboard
        // permission and stay off the UI thread. The same seam serves
        // ordinary paste, vim's register and in-app capture.
        .with_context(ui::Clipboard(std::sync::Arc::new(move || {
            let clipboard = clipboard.clone();
            Box::pin(async move { clipboard.read_text().await })
        })))
        // Ctrl+C's half of the clipboard: sent over the eval channel rather
        // than interpolated, so arbitrary note text cannot break the script
        // (adr/2026-08-hidden-ime-sink.md)
        .with_context(ui::ClipboardWrite(std::sync::Arc::new(|text| {
            Box::pin(async move {
                let eval = dioxus::document::eval(
                    "const text = await dioxus.recv(); \
                     await navigator.clipboard.writeText(text);",
                );
                let _ = eval.send(text);
                let _ = eval.await;
            })
        })))
        // where a mouse press landed, in a coordinate the editor speaks:
        // the hit span's data-start plus the UTF-16 offset within its text
        // node (adr/2026-08-caret-on-editor-note-bytes.md). Geometry stays
        // the webview's — the caret itself is app state.
        .with_context(ui::HitProbe(std::sync::Arc::new(|x, y| {
            Box::pin(async move {
                let eval = dioxus::document::eval(
                    "const [x, y] = await dioxus.recv(); \
                     const at = document.caretPositionFromPoint \
                         ? document.caretPositionFromPoint(x, y) \
                         : document.caretRangeFromPoint(x, y); \
                     if (!at) return null; \
                     const node = at.offsetNode ?? at.startContainer; \
                     const offset = at.offset ?? at.startOffset; \
                     const el = node.nodeType === Node.TEXT_NODE \
                         ? node.parentElement : node; \
                     const span = el && el.closest('[data-start]'); \
                     if (!span) return null; \
                     return [parseInt(span.dataset.start), offset];",
                );
                let _ = eval.send((x, y));
                eval.await.ok().and_then(|value| {
                    let pair = value.as_array()?;
                    let start = pair.first()?.as_u64()?;
                    let units = pair.get(1)?.as_u64()?;
                    Some((start as usize, units as usize))
                })
            })
        })))
        // where j/k land: the whole run walked inside the webview in one
        // round trip, each step probing a line height away from the rect of
        // the character the previous step landed on, restricted to the
        // active block. The app-drawn caret's rect seeds the walk and is
        // never read again — the DOM only flushes the caret's move after
        // this script has already been sent
        // (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md)
        .with_context(ui::LineProbe(std::sync::Arc::new(
            |goal, down, count| {
                Box::pin(async move {
                    let eval = dioxus::document::eval(LINE_WALK);
                    let _ = eval.send((goal, down, count));
                    eval.await.ok().and_then(|value| {
                        let quad = value.as_array()?;
                        Some(ui::Landing {
                            start: quad.first()?.as_u64()? as usize,
                            units: quad.get(1)?.as_u64()? as usize,
                            x: quad.get(2)?.as_f64()?,
                            taken: quad.get(3)?.as_u64()? as usize,
                        })
                    })
                })
            },
        )))
        .launch(ui::App)
}

/// The `[count]j`/`k` walk, run entirely inside the webview: `hit` reads
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
const LINE_WALK: &str = "\
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

/// Starts the vault watcher and bridges its blocking channel to the async
/// one the shell awaits. The spawned thread owns the watcher — dropping it
/// would stop the debouncer — and ends when the app closes the receiver.
fn watcher_feed(root: Option<&std::path::Path>) -> ui::VaultFeed {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let started = root.map(watch::VaultWatcher::start);
    match started {
        Some(Ok(watcher)) => {
            std::thread::spawn(move || {
                for batch in watcher.changes.iter() {
                    if sender.send(batch).is_err() {
                        break;
                    }
                }
            });
            feed(Some(receiver), None)
        }
        // the failure rides the feed into the status surface — a desktop
        // app's stderr is nowhere (adr/2026-08-status-surface-owns-notices.md)
        Some(Err(error)) => feed(None, Some(error.to_string())),
        None => feed(None, None),
    }
}

fn feed(
    receiver: Option<
        tokio::sync::mpsc::UnboundedReceiver<Vec<watch::VaultChange>>,
    >,
    trouble: Option<String>,
) -> ui::VaultFeed {
    ui::VaultFeed {
        changes: std::sync::Arc::new(std::sync::Mutex::new(receiver)),
        trouble,
    }
}
