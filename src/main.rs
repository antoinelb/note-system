#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use note_system::{clipboard, compute, launch, time, ui, vault};

// Only the desktop runtime can run this: every closure below calls into
// the window or the webview. What they decide lives in `launch`, inside
// the coverage gate; this function is the one the gate excuses, and any
// second function in this file would be counted
// (adr/2026-09-main-holds-only-the-launch-builder.md).
#[cfg_attr(coverage_nightly, coverage(off))]
fn main() {
    let root = vault::vault_path();

    // `wl-paste | app --capture` runs with no window: checked before the
    // window is built (adr/2026-08-capture-headless-second-process.md)
    if let Some(code) = launch::capture_cli(
        std::env::args().nth(1).as_deref(),
        root.clone(),
        &jiff::Zoned::now(),
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    ) {
        std::process::exit(code);
    }

    let clipboard = clipboard::native();

    dioxus::LaunchBuilder::new()
        .with_cfg(dioxus::desktop::Config::new().with_menu(None))
        .with_context(ui::VaultRoot(root.clone()))
        .with_context(ui::SeedTrouble(launch::seed_trouble(root.as_deref())))
        // the watcher's own thread forwards its batches into the async
        // channel the shell awaits (adr/2026-08-watcher-feeds-the-ui.md)
        .with_context(launch::watcher_feed(root.as_deref()))
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
        // the app's single clock source, re-read by every reader at its
        // own moment (adr/2026-07-today-injected-root-context.md,
        // adr/2026-09-the-clock-is-a-source-not-a-value.md)
        .with_context(ui::Today(time::clock()))
        // what a `#link` destination is handed to: the desktop's opener,
        // started and forgotten (adr/2026-09-link-is-for-resources.md)
        .with_context(ui::Launcher(std::sync::Arc::new(|target| {
            launch::open_with(launch::OPENER, target)
        })))
        // read again only when a capture is stamped, which is the one thing
        // that needs the time of day (adr/2026-08-capture-timestamp-ids.md)
        .with_context(ui::Now(std::sync::Arc::new(jiff::Zoned::now)))
        // Native reads bypass WebKitGTK's disabled JavaScript clipboard
        // permission and stay off the UI thread. The same seam serves
        // ordinary paste, vim's register and in-app capture.
        .with_context(ui::Clipboard(std::sync::Arc::new({
            let clipboard = clipboard.clone();
            move || {
                let clipboard = clipboard.clone();
                Box::pin(async move { clipboard.read_text().await })
            }
        })))
        // the same worker's image read, for a paste with no text on the
        // clipboard (adr/2026-09-an-image-pastes-into-assets.md)
        .with_context(ui::ClipboardImage(std::sync::Arc::new(move || {
            let clipboard = clipboard.clone();
            Box::pin(async move { clipboard.read_image().await })
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
        // where a mouse press landed, in a coordinate the editor speaks
        // (adr/2026-08-caret-on-editor-note-bytes.md)
        .with_context(ui::HitProbe(std::sync::Arc::new(|x, y| {
            Box::pin(async move {
                let eval = dioxus::document::eval(launch::HIT_PROBE);
                let _ = eval.send((x, y));
                eval.await.ok().and_then(|value| launch::hit(&value))
            })
        })))
        // where j/k land: the whole run walked inside the webview in one
        // round trip, seeded by the app-drawn caret's rect and never
        // reading it again — the DOM only flushes the caret's move after
        // this script has already been sent
        // (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md)
        .with_context(ui::LineProbe(std::sync::Arc::new(
            |goal, down, count| {
                Box::pin(async move {
                    let eval = dioxus::document::eval(launch::LINE_WALK);
                    let _ = eval.send((goal, down, count));
                    eval.await.ok().and_then(|value| launch::landing(&value))
                })
            },
        )))
        // where a freshly mounted caret puts itself in the pane: said in
        // the DOM's own words, because Dioxus's ScrollToOptions cannot say
        // it at all (adr/2026-09-the-caret-line-sits-at-the-centre.md)
        .with_context(ui::KeepFocus(std::sync::Arc::new(|| {
            // the script installs its listener the moment it is sent;
            // nothing comes back to await
            let _ = dioxus::document::eval(launch::KEEP_FOCUS);
        })))
        .with_context(ui::CaretScroll(std::sync::Arc::new(|block| {
            Box::pin(async move {
                let eval = dioxus::document::eval(launch::CARET_SCROLL);
                // a one-element tuple, so the script's `const [block]`
                // destructures an array — a bare string destructures to
                // its first character, and `scrollIntoView` refuses it
                let _ = eval.send((block,));
                let _ = eval.await;
            })
        })))
        .launch(ui::App)
}
