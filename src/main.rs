use note_system::{ui, vault};

fn main() {
    dioxus::LaunchBuilder::new()
        .with_cfg(dioxus::desktop::Config::new().with_menu(None))
        .with_context(ui::VaultRoot(vault::vault_path()))
        // called from the keydown handler, where the runtime context that
        // window() reads is current
        .with_context(ui::Closer(std::sync::Arc::new(|| {
            dioxus::desktop::window().close()
        })))
        .launch(ui::App)
}
