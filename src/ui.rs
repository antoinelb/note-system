use std::path::{Path, PathBuf};
use std::time::Duration;

use dioxus::prelude::*;

use crate::domain::NoteCategory;
use crate::editor::{Buffer, apply_edit};
use crate::index::{Index, IndexError};
use crate::render::SvgCache;

#[cfg(not(test))]
const QUIET: Duration = Duration::from_millis(500);
#[cfg(test)]
const QUIET: Duration = Duration::from_millis(1);

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
    let mut buffer = use_signal(|| None::<Buffer>);
    let _autosave = use_resource({
        let root = root.clone();
        move || {
            // reading the buffer here is what subscribes this resource to every
            // edit
            let _ = buffer.read();
            let root = root.clone();
            async move {
                tokio::time::sleep(QUIET).await;
                let flushed = buffer.with(|open| {
                    let note = open.as_ref()?;
                    Some(cache.with_mut(|cache| flush(&root, note, cache)))
                });
                if let Some(flushed) = flushed {
                    view.set(Some(flushed));
                }
            }
        }
    });
    rsx! {
        ul { class: "note-list",
            for entry in entries {
                li {
                    class: "note-entry",
                    onclick: {
                        let root = root.clone();
                        let path = entry.path.clone();
                        move |_| match Buffer::open(root.join(&path)) {
                            Ok(note) => {
                                let rendered = cache
                                    .with_mut(|cache| render_buffer(&root, &note, cache));
                                view.set(Some(rendered));
                                buffer.set(Some(note));
                            }
                            Err(err) => {
                                buffer.set(None);
                                view.set(Some(Err(format!(
                                    "{}: {err}",
                                    path.display()
                                ))));
                            }
                        }
                    },
                    "{entry.label}"
                }
            }
        }
        div { class: "editor",
            {
                let open = buffer.read();
                match open.as_ref() {
                    Some(note) => rsx! {
                        textarea {
                            key: "{note.file().display()}",
                            class: "source",
                            initial_value: "{note.text()}",
                            oninput: move |event| {
                                buffer.with_mut(|current| {
                                    apply_edit(current, event.value())
                                });
                            },
                        }
                    },
                    None => rsx! {},
                }
            }
            div { class: "rendered",
                {
                    match view() {
                        Some(Ok(svg)) => rsx! { div { dangerous_inner_html: "{svg}" } },
                        Some(Err(msg)) => rsx! { p { class: "render-error", "{msg}" } },
                        None => rsx! { p { "Select a note" } },
                    }
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

fn render_buffer(
    root: &Path,
    buffer: &Buffer,
    cache: &mut SvgCache,
) -> Result<String, String> {
    cache
        .render(root, buffer.file(), buffer.text())
        .map_err(|err| format!("{err:?}"))
}

fn flush(
    root: &Path,
    note: &Buffer,
    cache: &mut SvgCache,
) -> Result<String, String> {
    note.save()
        .map_err(|err| format!("{}: {err}", note.file().display()))?;
    render_buffer(root, note, cache)
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

    // -- the editor: buffer in, debounced save and recompile out ------------

    #[test]
    fn typing_saves_the_note_and_recompiles_it() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        let inputs = open_note(&mut dom, clicks[0]);
        assert_eq!(inputs.len(), 1, "the editor mounts exactly one textarea");

        type_into(&mut dom, inputs[0], &note("renamed"));

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("<svg"), "{html}");
        assert!(!html.contains("render-error"), "{html}");
        assert_eq!(
            std::fs::read_to_string(vault.path().join("permanent/alpha.typ"))
                .expect("the note is readable"),
            note("renamed"),
            "the debounced autosave reached disk"
        );
    }

    #[test]
    fn typing_something_that_will_not_compile_still_saves_it() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        let inputs = open_note(&mut dom, clicks[0]);

        type_into(&mut dom, inputs[0], "#let x = (\n");

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(!html.contains("<svg"), "{html}");
        // no hard blocks: a note that cannot compile is still the user's file
        assert_eq!(
            std::fs::read_to_string(vault.path().join("permanent/alpha.typ"))
                .expect("the note is readable"),
            "#let x = (\n"
        );
    }

    #[test]
    fn a_note_that_cannot_be_written_reports_the_failed_save() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        let inputs = open_note(&mut dom, clicks[0]);

        let note_path = vault.path().join("permanent/alpha.typ");
        let mut permissions = std::fs::metadata(&note_path)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&note_path, permissions)
            .expect("the note is made read-only");

        type_into(&mut dom, inputs[0], &note("renamed"));

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("alpha.typ"), "{html}");
    }

    #[test]
    fn the_autosave_writes_nothing_while_no_note_is_open() {
        let vault = temp_vault();
        let before =
            std::fs::read_to_string(vault.path().join("permanent/alpha.typ"))
                .expect("the note is readable");
        let (mut dom, _) = rendered_app(Some(vault.path().to_path_buf()));

        block_on(settle(&mut dom));

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("Select a note"), "{html}");
        assert_eq!(
            std::fs::read_to_string(vault.path().join("permanent/alpha.typ"))
                .expect("the note is readable"),
            before
        );
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
        let clicks = listeners(&mutations, "click");
        (dom, clicks)
    }

    fn listeners(mutations: &Mutations, wanted: &str) -> Vec<ElementId> {
        mutations
            .edits
            .iter()
            .filter_map(|edit| match edit {
                Mutation::NewEventListener { name, id } if name == wanted => {
                    Some(*id)
                }
                _ => None,
            })
            .collect()
    }

    fn click(dom: &mut VirtualDom, target: ElementId) {
        open_note(dom, target);
    }

    /// Clicks a note and returns the `input` listener the editor mounts for
    /// it, which is the handle the typing tests need.
    fn open_note(dom: &mut VirtualDom, target: ElementId) -> Vec<ElementId> {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedMouseData::default(),
            )));
            dom.runtime().handle_event(
                "click",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            listeners(&dom.render_immediate_to_vec(), "input")
        })
    }

    /// Types `text` into the editor and lets the debounced autosave finish,
    /// so assertions see the settled state rather than a half-run timer.
    fn type_into(dom: &mut VirtualDom, target: ElementId, text: &str) {
        block_on(async {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedFormData::new(text.to_string(), Vec::new()),
            )));
            dom.runtime().handle_event(
                "input",
                Event::new(data, true),
                target,
            );
            settle(dom).await;
        });
    }

    /// Drives the resource through its restart, its `QUIET` sleep and the
    /// write that follows. Bounded: four short waits, never a spin.
    async fn settle(dom: &mut VirtualDom) {
        for _ in 0..8 {
            let waited =
                tokio::time::timeout(QUIET * 50, dom.wait_for_work()).await;
            dom.render_immediate_to_vec();
            if waited.is_err() {
                break;
            }
        }
    }

    thread_local! {
        /// One runtime per test thread, never dropped mid-test: the autosave
        /// resource sleeps on a tokio timer, and a timer whose runtime has
        /// gone away panics with "context ... is being shutdown".
        /// `QUIET` is 1 ms under `cfg(test)`, so nothing here waits
        /// perceptibly.
        static REACTOR: tokio::runtime::Runtime =
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("a current-thread runtime builds");
    }

    /// Runs `work` with the thread's reactor in scope, for calls that poll
    /// dom tasks without awaiting anything themselves.
    fn with_reactor<T>(work: impl FnOnce() -> T) -> T {
        REACTOR.with(|reactor| {
            let _guard = reactor.enter();
            work()
        })
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        REACTOR.with(|reactor| reactor.block_on(future))
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

    /// Only mouse and form events are real: the shell listens for clicks on
    /// the note list and input on the editor, so every other conversion is
    /// unreachable in these tests.
    struct TestEvents;

    impl HtmlEventConverter for TestEvents {
        fn convert_mouse_data(&self, event: &PlatformEventData) -> MouseData {
            event
                .downcast::<SerializedMouseData>()
                .cloned()
                .map(MouseData::from)
                .expect("the tests only fire serialized mouse events")
        }

        fn convert_form_data(&self, event: &PlatformEventData) -> FormData {
            event
                .downcast::<SerializedFormData>()
                .cloned()
                .map(FormData::from)
                .expect("the tests only fire serialized form events")
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
