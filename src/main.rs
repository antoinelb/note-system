use note_system::{capture, time, ui, vault, watch};

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
        // called from the keydown handler, where the runtime context that
        // window() reads is current
        .with_context(ui::Closer(std::sync::Arc::new(|| {
            dioxus::desktop::window().close()
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
        // the boundary arrows' caret probe: selectionStart of the active
        // textarea, in the UTF-16 units the editor converts from
        // (adr/2026-07-hybrid-active-block-textarea.md)
        .with_context(ui::CaretProbe(std::sync::Arc::new(|| {
            Box::pin(async {
                dioxus::document::eval(
                    "const el = document.activeElement; \
                     return el && el.tagName === 'TEXTAREA' \
                         ? el.selectionStart : null;",
                )
                .await
                .ok()
                .and_then(|value| value.as_u64())
                .map(|units| units as usize)
            })
        })))
        // and back the other way: after an accepted completion the caret
        // belongs past the link it wrote
        // (adr/2026-08-ctrl-l-link-picker.md)
        .with_context(ui::CaretWriter(std::sync::Arc::new(|units| {
            Box::pin(async move {
                let _ = dioxus::document::eval(&format!(
                    "const el = document.activeElement; \
                     if (el && el.tagName === 'TEXTAREA') \
                         el.setSelectionRange({units}, {units});"
                ))
                .await;
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
            feed(Some(receiver))
        }
        Some(Err(error)) => {
            eprintln!("the vault will not be watched: {error}");
            feed(None)
        }
        None => feed(None),
    }
}

fn feed(
    receiver: Option<
        tokio::sync::mpsc::UnboundedReceiver<Vec<watch::VaultChange>>,
    >,
) -> ui::VaultFeed {
    ui::VaultFeed(std::sync::Arc::new(std::sync::Mutex::new(receiver)))
}
