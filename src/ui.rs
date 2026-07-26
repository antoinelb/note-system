use std::path::{Path, PathBuf};

use dioxus::prelude::*;

use crate::domain::NoteCategory;
use crate::index::{Index, IndexError};
use crate::render::SvgCache;

#[derive(Clone, Debug)]
pub struct VaultRoot(pub Option<PathBuf>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct NoteEntry {
    path: PathBuf,
    label: String,
}

#[component]
pub fn App() -> Element {
    let vault = use_context::<VaultRoot>();
    let loaded = use_hook(|| load(vault.0));
    match loaded {
        Ok((root, entries)) => rsx! { Shell {root,entries}},
        Err(msg) => rsx! { div { class: "vault-error", "{msg}"}},
    }
}

#[component]
fn Shell(root: PathBuf, entries: Vec<NoteEntry>) -> Element {
    let mut cache = use_signal(SvgCache::default);
    let mut view = use_signal(|| None::<Result<String, String>>);
    rsx! {
            ul {class: "note-list",
            for entry in entries {
                li {
                class: "note-entry",
                onclick: {
                    let root = root.clone();
                    let path = entry.path.clone();
                    move |_| {
                        let rendered = cache.with_mut(|cache| render_note(&root, &path, cache));
                        view.set(Some(rendered));
                    }
                },
                "{entry.label}"
            }
            }
        }
        div {class: "viewer", {
            match view() {
            Some(Ok(svg)) => rsx! { div {dangerous_inner_html: "{svg}"}},
            Some(Err(msg)) => rsx! {p { class: "render-error", "{msg}"}},
            None => rsx! { p { "Select a note"}}
        }
        }
    }
    }
}

fn load(root: Option<PathBuf>) -> Result<(PathBuf, Vec<NoteEntry>), String> {
    match root {
        Some(root) => match load_notes(&root) {
            Ok(entries) => Ok((root, entries)),
            Err(err) => Err(format!("the index could not be built: {err:?}")),
        },
        None => Err("no vault: define NOTE_VAULT or HOME".to_string()),
    }
}

fn load_notes(root: &Path) -> Result<Vec<NoteEntry>, IndexError> {
    let notes = crate::index::scan_vault(root)?;
    let index_path = root.join(".index");
    std::fs::create_dir_all(&index_path)?;
    let mut index = Index::open(&index_path.join("index.db"))?;
    index.rebuild(&notes)?;
    list_entries(&index)
}

fn list_entries(index: &Index) -> Result<Vec<NoteEntry>, IndexError> {
    let mut entries = Vec::new();
    for category in [
        NoteCategory::Permanent,
        NoteCategory::Time,
        NoteCategory::Capture,
        NoteCategory::Generated,
    ] {
        for path in index.notes_by_category(&category)? {
            entries.push(NoteEntry {
                label: path
                    .file_stem()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                path,
            });
        }
    }
    Ok(entries)
}

fn render_note(
    root: &Path,
    note: &Path,
    cache: &mut SvgCache,
) -> Result<String, String> {
    let path = root.join(note);
    let text = std::fs::read_to_string(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    cache
        .render(root, &path, &text)
        .map_err(|err| format!("{err:?}"))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::any::Any;
    use std::rc::Rc;

    use dioxus::dioxus_core::{ElementId, Event, Mutation, Mutations};
    use dioxus::html::*;
    use dioxus::prelude::VirtualDom;

    use super::*;

    // -- the App component, driven headlessly through a VirtualDom ----------

    #[test]
    fn without_a_vault_the_app_shows_the_vault_error() {
        let (dom, clicks) = rendered_app(None);
        let html = dioxus_ssr::render(&dom);
        assert!(clicks.is_empty(), "{html}");
        assert!(html.contains("vault-error"), "{html}");
        assert!(html.contains("no vault: define NOTE_VAULT or HOME"));
    }

    #[test]
    fn an_unbuildable_index_shows_the_vault_error() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let (dom, _) = rendered_app(Some(dir.path().join("missing")));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("vault-error"), "{html}");
        assert!(html.contains("the index could not be built"), "{html}");
    }

    #[test]
    fn the_note_list_has_one_click_listener_per_note_in_index_order() {
        let vault = temp_vault();
        let (dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);

        assert_eq!(clicks.len(), 3, "{html}");
        let alpha = html.find("alpha").expect("alpha is listed");
        let omega = html.find("omega").expect("omega is listed");
        let broken = html.find("broken").expect("broken is listed");
        assert!(alpha < omega && omega < broken, "{html}");

        // nothing is rendered before a click
        assert!(!html.contains("<svg"), "{html}");
        assert!(html.contains("Select a note"), "{html}");
    }

    #[test]
    fn clicking_a_note_shows_its_rendered_svg() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("<svg"), "{html}");
    }

    #[test]
    fn clicking_a_broken_note_shows_the_render_error() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[2]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(!html.contains("<svg"), "{html}");
    }

    #[test]
    fn clicking_a_vanished_note_shows_the_render_error() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join("permanent/alpha.typ"))
            .expect("the note exists before the click");
        click(&mut dom, clicks[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("alpha.typ"), "{html}");
    }

    // -- load_notes: the happy path and every error edge --------------------

    #[test]
    fn load_notes_lists_categories_in_fixed_order_then_paths() {
        let vault = temp_vault();
        let entries =
            load_notes(vault.path()).expect("the temp vault indexes");
        let paths: Vec<&Path> =
            entries.iter().map(|entry| entry.path.as_path()).collect();
        assert_eq!(
            paths,
            [
                Path::new("permanent/alpha.typ"),
                Path::new("permanent/omega.typ"),
                Path::new("capture/broken.typ"),
            ]
        );
        let labels: Vec<&str> =
            entries.iter().map(|entry| entry.label.as_str()).collect();
        assert_eq!(labels, ["alpha", "omega", "broken"]);
    }

    #[test]
    fn a_missing_vault_fails_at_the_scan() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let error = load_notes(&dir.path().join("missing")).unwrap_err();
        assert!(matches!(error, IndexError::Io(_)), "{error:?}");
    }

    #[test]
    fn a_file_squatting_the_index_directory_fails_at_creation() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        std::fs::write(dir.path().join(".index"), "not a directory")
            .expect("the squatting file is written");
        let error = load_notes(dir.path()).unwrap_err();
        assert!(matches!(error, IndexError::Io(_)), "{error:?}");
    }

    #[test]
    fn a_directory_squatting_the_database_fails_at_open() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        std::fs::create_dir_all(dir.path().join(".index/index.db"))
            .expect("the squatting directory is created");
        let error = load_notes(dir.path()).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_read_only_database_fails_at_the_rebuild() {
        let vault = temp_vault();
        load_notes(vault.path()).expect("the first build succeeds");
        let db = vault.path().join(".index/index.db");
        let mut permissions = std::fs::metadata(&db)
            .expect("the database exists after the first build")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&db, permissions)
            .expect("the database is made read-only");
        let error = load_notes(vault.path()).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_sabotaged_index_fails_at_the_listing() {
        let vault = temp_vault();
        let notes = crate::index::scan_vault(vault.path())
            .expect("the temp vault scans");
        let index_dir = vault.path().join(".index");
        std::fs::create_dir_all(&index_dir)
            .expect("the index directory is created");
        let mut index = Index::open(&index_dir.join("index.db"))
            .expect("the database opens");
        index.rebuild(&notes).expect("the rebuild succeeds");

        let saboteur = rusqlite::Connection::open(index_dir.join("index.db"))
            .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE notes")
            .expect("the sabotage succeeds");

        let error = list_entries(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    // -- harness -------------------------------------------------------------

    /// Builds the App headlessly with the vault injected as root context —
    /// the same channel `main` uses — and returns the click targets found in
    /// the initial mutations, in document order.
    fn rendered_app(root: Option<PathBuf>) -> (VirtualDom, Vec<ElementId>) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        let mutations = dom.rebuild_to_vec();
        let clicks = click_targets(&mutations);
        (dom, clicks)
    }

    fn click_targets(mutations: &Mutations) -> Vec<ElementId> {
        mutations
            .edits
            .iter()
            .filter_map(|edit| match edit {
                Mutation::NewEventListener { name, id } if name == "click" => {
                    Some(*id)
                }
                _ => None,
            })
            .collect()
    }

    fn click(dom: &mut VirtualDom, target: ElementId) {
        let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
            SerializedMouseData::default(),
        )));
        dom.runtime()
            .handle_event("click", Event::new(data, true), target);
        dom.process_events();
        dom.render_immediate_to_vec();
    }

    /// A vault with two valid permanent notes and one capture note whose
    /// body cannot compile — enough to exercise list order, SVG rendering
    /// and the render-error path.
    fn temp_vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let root = dir.path();
        for (path, text) in [
            (
                "templates/template.typ",
                concat!(
                    "#let meta(id: none, type: none, created: none, ",
                    "tags: (), origin: none) = []\n",
                    "#let l(id) = [#id]\n",
                    "#let note(doc) = doc\n",
                )
                .to_string(),
            ),
            ("permanent/alpha.typ", note("alpha")),
            ("permanent/omega.typ", note("omega")),
            ("capture/broken.typ", "#let x = (\n".to_string()),
        ] {
            let path = root.join(path);
            std::fs::create_dir_all(
                path.parent().expect("vault files sit in a category"),
            )
            .expect("the category directory is created");
            std::fs::write(path, text).expect("the note is written");
        }
        dir
    }

    fn note(id: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{id}\", type: \"concept\", created: \"2026-07-01\")\n\
             \n= {id}\n"
        )
    }

    /// Only mouse events are real: the shell listens for nothing else, so
    /// every other conversion is unreachable in these tests.
    struct TestEvents;

    impl HtmlEventConverter for TestEvents {
        fn convert_mouse_data(&self, event: &PlatformEventData) -> MouseData {
            event
                .downcast::<SerializedMouseData>()
                .cloned()
                .map(MouseData::from)
                .expect("the tests only fire serialized mouse events")
        }

        fn convert_animation_data(
            &self,
            _: &PlatformEventData,
        ) -> AnimationData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_cancel_data(&self, _: &PlatformEventData) -> CancelData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_clipboard_data(
            &self,
            _: &PlatformEventData,
        ) -> ClipboardData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_composition_data(
            &self,
            _: &PlatformEventData,
        ) -> CompositionData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_drag_data(&self, _: &PlatformEventData) -> DragData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_focus_data(&self, _: &PlatformEventData) -> FocusData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_form_data(&self, _: &PlatformEventData) -> FormData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_image_data(&self, _: &PlatformEventData) -> ImageData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_keyboard_data(
            &self,
            _: &PlatformEventData,
        ) -> KeyboardData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_media_data(&self, _: &PlatformEventData) -> MediaData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_mounted_data(&self, _: &PlatformEventData) -> MountedData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_pointer_data(&self, _: &PlatformEventData) -> PointerData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_resize_data(&self, _: &PlatformEventData) -> ResizeData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_scroll_data(&self, _: &PlatformEventData) -> ScrollData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_selection_data(
            &self,
            _: &PlatformEventData,
        ) -> SelectionData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_toggle_data(&self, _: &PlatformEventData) -> ToggleData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_touch_data(&self, _: &PlatformEventData) -> TouchData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_transition_data(
            &self,
            _: &PlatformEventData,
        ) -> TransitionData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_visible_data(&self, _: &PlatformEventData) -> VisibleData {
            unreachable!("the shell only listens for clicks")
        }

        fn convert_wheel_data(&self, _: &PlatformEventData) -> WheelData {
            unreachable!("the shell only listens for clicks")
        }
    }
}
