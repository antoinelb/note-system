use note_system::{ui, vault};

fn main() {
    dioxus::LaunchBuilder::new()
        .with_context(ui::VaultRoot(vault::vault_path()))
        .launch(ui::App)
}
