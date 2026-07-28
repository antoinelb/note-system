use note_system::{ui, vault};

fn main() {
    dioxus::LaunchBuilder::new()
        .with_cfg(dioxus::desktop::Config::new().with_menu(None))
        .with_context(ui::VaultRoot(vault::vault_path()))
        .launch(ui::App)
}
