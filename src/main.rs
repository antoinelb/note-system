use note_system::{capture, time, ui, vault};

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

    dioxus::LaunchBuilder::new()
        .with_cfg(dioxus::desktop::Config::new().with_menu(None))
        .with_context(ui::VaultRoot(vault::vault_path()))
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
