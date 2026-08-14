use note_system::{capture, compute, time, ui, vault, watch};

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

    dioxus::LaunchBuilder::new()
        .with_cfg(dioxus::desktop::Config::new().with_menu(None))
        .with_context(ui::VaultRoot(root.clone()))
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
        // in-app capture reads what is on the clipboard; a webview that
        // refuses the read captures nothing rather than an empty note
        .with_context(ui::Clipboard(std::sync::Arc::new(|| {
            Box::pin(async {
                dioxus::document::eval(
                    "return await navigator.clipboard.readText();",
                )
                .await
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
            })
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
        .launch(ui::App)
}

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
