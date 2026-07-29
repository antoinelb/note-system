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
        .launch(ui::App)
}
