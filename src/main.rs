use note_system::{time, ui, vault};

fn main() {
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
