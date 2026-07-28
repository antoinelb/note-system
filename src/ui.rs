use std::path::{Path, PathBuf};
use std::time::Duration;

use dioxus::prelude::*;

use crate::domain::{NoteCategory, NoteType};
use crate::editor::{Buffer, apply_edit};
use crate::index::{Index, IndexError};
use crate::render::SvgCache;

#[cfg(not(test))]
const QUIET: Duration = Duration::from_millis(500);
#[cfg(test)]
const QUIET: Duration = Duration::from_millis(1);

const PERMANENT_TYPES: [&str; 8] = [
    "person",
    "organisation",
    "source",
    "concept",
    "claim",
    "idea",
    "personal",
    "project",
];

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
    let mut light = use_signal(|| false);
    rsx! {
        document::Stylesheet { href: asset!("/assets/theme.css") }
        div {
            class: "app",
            // always "dark" or "light", never absent: the theme is a fact of
            // the tree, not an absence to interpret
            // (adr/2026-07-theme-attribute-on-app-root.md)
            "data-theme": if light() { "light" } else { "dark" },
            // focusable so the theme keystroke lands here; keydowns from the
            // editor bubble up, so the chord also works while writing
            tabindex: "0",
            autofocus: true,
            onkeydown: move |event| {
                if event.modifiers().ctrl()
                    && event.key() == Key::Character("t".to_string())
                {
                    light.set(!light());
                }
            },
            {
                match loaded {
                    Ok((root, entries, loops)) => {
                        rsx! { Shell { root, entries, loops } }
                    }
                    Err(msg) => rsx! { div { class: "vault-error", "{msg}" } },
                }
            }
        }
    }
}

#[component]
fn Shell(root: PathBuf, entries: Vec<NoteEntry>, loops: usize) -> Element {
    let mut entries = use_signal(|| entries);
    let mut loops = use_signal(|| loops);

    let mut cache = use_signal(SvgCache::default);
    let mut view = use_signal(|| None::<Result<String, String>>);
    let mut buffer = use_signal(|| None::<Buffer>);

    let mut new_type = use_signal(|| "concept".to_string());
    let mut new_title = use_signal(String::new);

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
               Chrome { screen: Screen::Logs, loops: loops() }
               ul { class: "note-list",
                   for entry in entries() {
                       li {
                           class: "note-entry",
                           onclick: {
                               let root = root.clone();
                               let path = entry.path.clone();
                       move |_| open_note(&root,root.join(&path),cache,view,buffer)                    },
                           "{entry.label}"
                       }
                   }
               }
           div {
       class: "create-note",
           select {
           class: "create-type type-label",
           onchange: move |event| new_type.set(event.value()),
           for name in PERMANENT_TYPES {
           option { value: "{name}", selected: name == new_type(), "{name}"}
       }
       }
    input {
           class: "create-title",
           placeholder: "New note title",
           value: "{new_title}",
           oninput: move |event| new_title.set(event.value()),
       }
       button {
           class: "create-button",
           onclick: {
               let root = root.clone();
               move |_| {
                   let note_type = NoteType::from_name(&new_type());
                   match crate::template::create(
                       &root,
                       &NoteCategory::Permanent,
                       &note_type,
                       &new_title(),
                       &today(),
                       "",
                   ) {
                       Ok(path) => {
                           entries.with_mut(|list| list.push(entry_for(path.clone())));
                           new_title.set(String::new());
                           open_note(&root, path, cache, view, buffer);
                       }
                       Err(err) => {
                           view.set(Some(Err(format!("create: {err:?}"))));
                       }
                   }
               }
           },
           "Create"
       }
       button {
           class: "today-button",
           onclick: {
               let root = root.clone();
               move |_| match daily_note(&root, &today()) {
                   Ok((path, created)) => {
                       if created {
                           entries.with_mut(|list| {
                               list.push(entry_for(path.clone()))
                           });
                       }
                       open_note(&root, path, cache, view, buffer);
                   }
                   Err(err) => {
                       view.set(Some(Err(format!("today: {err:?}"))));
                   }
               }
           },
           "Today"
       }
       button {
           class: "delete-button",
           onclick: {
               let root = root.clone();
               move |_| {
                   let Some(file) = buffer.with(|open| {
                       open.as_ref().map(|note| note.file().to_path_buf())
                   }) else {
                       return; // nothing open, nothing to delete
                   };
                   match delete_note(&root, &file) {
                       Ok(count) => {
                           // entry paths may be vault-relative or absolute;
                           // join is a no-op on absolutes, so the comparison
                           // handles both
                           entries.with_mut(|list| {
                               list.retain(|entry| {
                                   root.join(&entry.path) != file
                               })
                           });
                           // clearing the buffer also parks the pending
                           // autosave: it re-reads the buffer at flush time,
                           // so nothing resurrects the deleted file
                           buffer.set(None);
                           view.set(None);
                           loops.set(count);
                       }
                       Err(msg) => view.set(Some(Err(msg))),
                   }
               }
           },
           "Delete"
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

/// v0 has one screen — the proto-logs list — so the table icon stays dim
/// until v1 mounts it; neither icon navigates before the phase-7 logs screen.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Table,
    Logs,
}

/// The one-line chrome (design § Chrome): two 14×14 stroked icons, the
/// current screen's lit, and the open-loops ember. Zero loops renders
/// nothing at all — absence, not a zero.
#[component]
fn Chrome(screen: Screen, loops: usize) -> Element {
    rsx! {
        header { class: "chrome",
            svg {
                class: if screen == Screen::Table { "icon-table lit" } else { "icon-table" },
                width: "14",
                height: "14",
                view_box: "0 0 14 14",
                rect { x: "1", y: "2", width: "5", height: "4", fill: "none", stroke: "currentColor" }
                rect { x: "8", y: "5", width: "5", height: "4", fill: "none", stroke: "currentColor" }
                rect { x: "3", y: "9", width: "5", height: "4", fill: "none", stroke: "currentColor" }
            }
            svg {
                class: if screen == Screen::Logs { "icon-logs lit" } else { "icon-logs" },
                width: "14",
                height: "14",
                view_box: "0 0 14 14",
                rect { x: "1.5", y: "2.5", width: "11", height: "10", fill: "none", stroke: "currentColor" }
                line { x1: "1.5", y1: "5.5", x2: "12.5", y2: "5.5", stroke: "currentColor" }
                line { x1: "4.5", y1: "1", x2: "4.5", y2: "3.5", stroke: "currentColor" }
                line { x1: "9.5", y1: "1", x2: "9.5", y2: "3.5", stroke: "currentColor" }
            }
            if loops > 0 {
                span { class: "ember", "{loops}" }
            }
        }
    }
}

fn load(
    root: Option<PathBuf>,
) -> Result<(PathBuf, Vec<NoteEntry>, usize), String> {
    match root {
        Some(root) => match load_notes(&root) {
            Ok((entries, loops)) => Ok((root, entries, loops)),
            Err(err) => Err(format!("the index could not be built: {err:?}")),
        },
        None => Err("no vault: define NOTE_VAULT or HOME".to_string()),
    }
}

fn load_notes(root: &Path) -> Result<(Vec<NoteEntry>, usize), IndexError> {
    let notes = crate::index::scan_vault(root)?;
    let index_path = root.join(".index");
    std::fs::create_dir_all(&index_path)?;
    let mut index = Index::open(&index_path.join("index.db"))?;
    index.rebuild(&notes)?;
    survey(&index)
}

fn survey(index: &Index) -> Result<(Vec<NoteEntry>, usize), IndexError> {
    Ok((list_entries(index)?, loop_count(index)?))
}

/// The v0 open-loops count: typeless notes + dangling links
/// (adr/2026-07-debt-counter-then-list.md). Unsummarized captures join in
/// phase 10, which also moves the count onto the watcher; until then delete
/// is the one in-session action that can change it.
fn loop_count(index: &Index) -> Result<usize, IndexError> {
    Ok(index.typeless_notes()?.len() + index.dangling_links()?.len())
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
            entries.push(entry_for(path));
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

fn open_note(
    root: &Path,
    file: PathBuf,
    mut cache: Signal<SvgCache>,
    mut view: Signal<Option<Result<String, String>>>,
    mut buffer: Signal<Option<Buffer>>,
) {
    match Buffer::open(file.clone()) {
        Ok(note) => {
            let rendered =
                cache.with_mut(|cache| render_buffer(root, &note, cache));
            view.set(Some(rendered));
            buffer.set(Some(note));
        }
        Err(err) => {
            buffer.set(None);
            view.set(Some(Err(format!("{}: {err}", file.display()))));
        }
    }
}

/// Today's daily note: created from the template when missing, reused when
/// present — `AlreadyExists` carries the existing path, so "already there"
/// is an answer here, not an error. The bool reports whether a file was
/// written, so the caller knows to add a list entry.
fn daily_note(
    root: &Path,
    date: &str,
) -> Result<(PathBuf, bool), crate::template::TemplateError> {
    // the id IS the date: kebab_id leaves "2026-07-27" unchanged
    match crate::template::create(
        root,
        &NoteCategory::Time,
        &NoteType::Daily,
        date,
        date,
        "",
    ) {
        Ok(path) => Ok((path, true)),
        Err(crate::template::TemplateError::AlreadyExists(path)) => {
            Ok((path, false))
        }
        Err(err) => Err(err),
    }
}

/// Deletes the note on disk, then forgets it in the index kept under the
/// vault, so the running session matches what the next launch's rebuild
/// would compute. Dangling links the deletion causes are the index's job
/// to surface, not this function's — it only reports the refreshed loop
/// count, since a delete is what can change it.
fn delete_note(root: &Path, file: &Path) -> Result<usize, String> {
    std::fs::remove_file(file)
        .map_err(|err| format!("{}: {err}", file.display()))?;
    // the index stores vault-relative paths; for a file outside the vault
    // the full path simply matches no row, and remove_note is a no-op
    let relative = file.strip_prefix(root).unwrap_or(file);
    let mut index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("{}: {err:?}", file.display()))?;
    index
        .remove_note(relative)
        .and_then(|()| loop_count(&index))
        .map_err(|err| format!("{}: {err:?}", file.display()))
}

/// The path may be vault-relative (from the index) or absolute (from a
/// create) — the click handler's `root.join` is a no-op for absolute paths.
fn entry_for(path: PathBuf) -> NoteEntry {
    NoteEntry {
        label: path
            .file_stem()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        path,
    }
}

/// The one clock read in the app: creation dates enter at this edge, so
/// everything below it stays deterministic (`created` is always a parameter).
fn today() -> String {
    jiff::Zoned::now().date().to_string()
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

    /// Only a typst-rendered note carries the SVG namespace — the chrome's
    /// rsx icons don't — so this is the "a note is rendered" marker.
    const RENDERED_NOTE: &str = r#"xmlns="http://www.w3.org/2000/svg""#;

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

    // -- the theme: one keystroke, one attribute -----------------------------

    #[test]
    fn ctrl_t_toggles_the_theme_and_back() {
        let (mut dom, keydown) = theme_app();
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="dark""#), "{html}");

        press(
            &mut dom,
            keydown,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="light""#), "{html}");

        press(
            &mut dom,
            keydown,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="dark""#), "{html}");
    }

    #[test]
    fn other_keys_leave_the_theme_alone() {
        let (mut dom, keydown) = theme_app();
        // a bare t (no modifier) and a chord on the wrong key: neither half
        // of the Ctrl+T check may fire alone
        press(
            &mut dom,
            keydown,
            Key::Character("t".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            keydown,
            Key::Character("x".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="dark""#), "{html}");
    }

    // -- the chrome: two icons and the ember ---------------------------------

    #[test]
    fn the_lit_icon_follows_the_current_screen() {
        for (screen, lit, dim) in [
            (Screen::Table, "icon-table lit", "icon-logs lit"),
            (Screen::Logs, "icon-logs lit", "icon-table lit"),
        ] {
            let mut dom = VirtualDom::new_with_props(
                Chrome,
                ChromeProps { screen, loops: 0 },
            );
            dom.rebuild_to_vec();
            let html = dioxus_ssr::render(&dom);
            assert!(html.contains(lit), "{html}");
            assert!(!html.contains(dim), "{html}");
            assert!(!html.contains("ember"), "zero renders nothing: {html}");
        }
    }

    #[test]
    fn the_ember_is_absent_when_no_loops_are_open() {
        let vault = temp_vault();
        let (dom, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("ember"), "absence, not a zero: {html}");
    }

    #[test]
    fn the_ember_shows_the_open_loop_count() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/linky.typ"),
            format!("{}#l(\"ghost\")\n", note("linky")),
        )
        .expect("the dangling note is written");
        let (dom, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"<span class="ember">1</span>"#), "{html}");
    }

    #[test]
    fn deleting_the_last_loop_extinguishes_the_ember() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/linky.typ"),
            format!("{}#l(\"ghost\")\n", note("linky")),
        )
        .expect("the dangling note is written");
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"<span class="ember">1</span>"#), "{html}");

        // permanent notes list alphabetically — alpha, linky, omega — and
        // the delete button is the last click listener
        click(&mut dom, clicks[1]);
        click(&mut dom, clicks[6]);

        let html = dioxus_ssr::render(&dom);
        assert!(!vault.path().join("permanent/linky.typ").exists());
        assert!(!html.contains("ember"), "the reward is absence: {html}");
    }

    #[test]
    fn the_note_list_has_one_click_listener_per_note_in_index_order() {
        let vault = temp_vault();
        let (dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);

        assert_eq!(
            clicks.len(),
            6,
            "three notes, then the create, today and delete buttons: {html}"
        );
        let alpha = html.find("alpha").expect("alpha is listed");
        let omega = html.find("omega").expect("omega is listed");
        let broken = html.find("broken").expect("broken is listed");
        assert!(alpha < omega && omega < broken, "{html}");

        // the create bar is mounted after the list
        assert!(html.contains("New note title"), "{html}");

        // nothing is rendered before a click
        assert!(!html.contains(RENDERED_NOTE), "{html}");
        assert!(html.contains("Select a note"), "{html}");
    }

    #[test]
    fn clicking_a_note_shows_its_rendered_svg() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn clicking_a_broken_note_shows_the_render_error() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[2]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(!html.contains(RENDERED_NOTE), "{html}");
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
        assert!(html.contains(RENDERED_NOTE), "{html}");
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
        assert!(!html.contains(RENDERED_NOTE), "{html}");
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

    // -- the create bar: type + title in, a filled note out ------------------

    #[test]
    fn creating_a_note_fills_the_template_and_opens_it() {
        let vault = temp_vault();
        let (mut dom, clicks, inputs, _) =
            mounted_app(Some(vault.path().to_path_buf()));
        assert_eq!(inputs.len(), 1, "the create bar mounts one title input");

        type_into(&mut dom, inputs[0], "Deep Modules");
        click(&mut dom, clicks[3]);

        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(RENDERED_NOTE),
            "the new note opens rendered: {html}"
        );
        assert!(
            html.contains("deep-modules"),
            "the new note is listed: {html}"
        );
        let written = std::fs::read_to_string(
            vault.path().join("permanent/deep-modules.typ"),
        )
        .expect("the created note reached disk");
        assert!(written.contains("= Deep Modules"), "{written}");
        assert!(written.contains(&today()), "{written}");
        assert!(
            !written.contains("{{"),
            "every placeholder is filled: {written}"
        );
    }

    #[test]
    fn a_duplicate_title_is_an_error_not_a_second_file() {
        let vault = temp_vault();
        let (mut dom, clicks, inputs, _) =
            mounted_app(Some(vault.path().to_path_buf()));

        type_into(&mut dom, inputs[0], "Twice");
        click(&mut dom, clicks[3]);
        type_into(&mut dom, inputs[0], "Twice");
        click(&mut dom, clicks[3]);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("AlreadyExists"), "{html}");
    }

    #[test]
    fn a_type_without_a_template_reports_the_create_error() {
        let vault = temp_vault();
        let (mut dom, clicks, inputs, changes) =
            mounted_app(Some(vault.path().to_path_buf()));
        assert_eq!(changes.len(), 1, "the create bar mounts one type select");

        change_type(&mut dom, changes[0], "idea");
        type_into(&mut dom, inputs[0], "An Idea");
        click(&mut dom, clicks[3]);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("UnknownTemplate"), "{html}");
        assert!(!vault.path().join("permanent/an-idea.typ").exists());
    }

    // -- the today action: create or open today's daily note -----------------

    #[test]
    fn daily_note_creates_once_then_reuses_without_touching_the_file() {
        let vault = temp_vault();
        let (path, created) =
            daily_note(vault.path(), "2026-07-27").expect("first call");
        assert!(created);
        assert_eq!(path, vault.path().join("time/2026-07-27.typ"));

        std::fs::write(&path, "= edited by hand\n").expect("edit the note");
        let (again, created) =
            daily_note(vault.path(), "2026-07-27").expect("second call");
        assert!(!created, "already there is an answer, not a write");
        assert_eq!(again, path);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read the note"),
            "= edited by hand\n"
        );
    }

    #[test]
    fn daily_note_passes_other_errors_through() {
        let vault = temp_vault();
        std::fs::remove_file(vault.path().join("templates/daily.typ"))
            .expect("remove the template");
        let result = daily_note(vault.path(), "2026-07-27");
        assert!(matches!(
            result,
            Err(crate::template::TemplateError::UnknownTemplate(name))
                if name == "daily"
        ));
    }

    #[test]
    fn clicking_today_creates_and_opens_the_daily_note() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));

        click(&mut dom, clicks[4]);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "today opens rendered: {html}");
        assert!(html.contains(&today()), "today is listed: {html}");
        let written = std::fs::read_to_string(
            vault.path().join(format!("time/{}.typ", today())),
        )
        .expect("today's note reached disk");
        assert!(
            !written.contains("{{"),
            "every placeholder filled: {written}"
        );
    }

    #[test]
    fn clicking_today_twice_opens_the_existing_note_once_listed() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));

        click(&mut dom, clicks[4]);
        click(&mut dom, clicks[4]);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "{html}");
        assert!(!html.contains("render-error"), "{html}");
        // the date also sits in the textarea source, so count list entries
        assert_eq!(
            html.matches(&format!(">{}</li>", today())).count(),
            1,
            "one list entry, not one per click: {html}"
        );
    }

    #[test]
    fn a_missing_daily_template_reports_the_today_error() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join("templates/daily.typ"))
            .expect("remove the template");

        click(&mut dom, clicks[4]);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("UnknownTemplate"), "{html}");
        assert!(!vault.path().join(format!("time/{}.typ", today())).exists());
    }

    // -- delete: file and index rows gone, debt surfaces elsewhere -----------

    #[test]
    fn delete_note_removes_the_file_and_its_index_rows() {
        let vault = temp_vault();
        load_notes(vault.path()).expect("build the index");
        let file = vault.path().join("permanent/alpha.typ");

        delete_note(vault.path(), &file).expect("delete");

        assert!(!file.exists());
        let index = Index::open(&vault.path().join(".index/index.db"))
            .expect("reopen the index");
        assert!(
            !index
                .notes_by_category(&NoteCategory::Permanent)
                .expect("query")
                .contains(&PathBuf::from("permanent/alpha.typ")),
            "the running index matches what a rebuild would compute"
        );
    }

    #[test]
    fn delete_note_reports_a_file_that_is_not_there() {
        let vault = temp_vault();
        let file = vault.path().join("permanent/ghost.typ");
        let result = delete_note(vault.path(), &file);
        assert!(matches!(result, Err(msg) if msg.contains("ghost.typ")));
    }

    #[test]
    fn delete_note_reports_an_unopenable_index() {
        // no load_notes: the .index directory was never created
        let vault = temp_vault();
        let file = vault.path().join("permanent/alpha.typ");
        let result = delete_note(vault.path(), &file);
        assert!(matches!(result, Err(msg) if msg.contains("alpha.typ")));
        assert!(!file.exists(), "the file half still happened");
    }

    #[test]
    fn delete_note_reports_an_index_it_cannot_write() {
        let vault = temp_vault();
        load_notes(vault.path()).expect("build the index");
        let db = vault.path().join(".index/index.db");
        let mut permissions = std::fs::metadata(&db)
            .expect("stat the database")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&db, permissions)
            .expect("make the database read-only");

        let file = vault.path().join("permanent/alpha.typ");
        let result = delete_note(vault.path(), &file);
        assert!(matches!(result, Err(msg) if msg.contains("alpha.typ")));
    }

    #[test]
    fn delete_note_outside_the_vault_touches_no_index_row() {
        // the strip_prefix fallback: a full path matches no relative row
        let vault = temp_vault();
        load_notes(vault.path()).expect("build the index");
        let outside = tempfile::tempdir().expect("a sibling temp dir");
        let file = outside.path().join("stray.typ");
        std::fs::write(&file, "= stray\n").expect("write the stray file");

        delete_note(vault.path(), &file).expect("delete");

        assert!(!file.exists());
        let index = Index::open(&vault.path().join(".index/index.db"))
            .expect("reopen the index");
        assert_eq!(
            index
                .notes_by_category(&NoteCategory::Permanent)
                .expect("query")
                .len(),
            2,
            "no vault row was harmed"
        );
    }

    #[test]
    fn deleting_the_open_note_removes_it_everywhere() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[0]);

        click(&mut dom, clicks[5]);

        let html = dioxus_ssr::render(&dom);
        assert!(!vault.path().join("permanent/alpha.typ").exists());
        assert!(!html.contains("alpha"), "gone from the list too: {html}");
        assert!(html.contains("Select a note"), "the pane is empty: {html}");
        assert!(!html.contains("textarea"), "{html}");
    }

    #[test]
    fn delete_with_nothing_open_does_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));

        click(&mut dom, clicks[5]);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("alpha"), "{html}");
        assert!(html.contains("Select a note"), "{html}");
        assert!(vault.path().join("permanent/alpha.typ").exists());
    }

    #[test]
    fn a_delete_that_fails_reports_the_error() {
        let vault = temp_vault();
        let (mut dom, clicks) = rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[0]);
        std::fs::remove_file(vault.path().join("permanent/alpha.typ"))
            .expect("pull the file out from under the app");

        click(&mut dom, clicks[5]);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("alpha.typ"), "{html}");
    }

    // -- load_notes: the happy path and every error edge --------------------

    #[test]
    fn load_notes_lists_categories_in_fixed_order_then_paths() {
        let vault = temp_vault();
        let (entries, loops) =
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
        // broken.typ is a capture: a capture without a type is phase-10
        // "unsummarized" debt, not a typeless note, so nothing is open here
        assert_eq!(loops, 0);
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

    #[test]
    fn a_sabotaged_notes_table_fails_the_survey_and_the_count() {
        let vault = temp_vault();
        let index = sabotaged_index(vault.path(), "DROP TABLE notes");
        let error = survey(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
        let error = loop_count(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_sabotaged_links_table_fails_the_survey_count() {
        // the listing half survives on the notes table; the count is what
        // reaches the links table and fails
        let vault = temp_vault();
        let index = sabotaged_index(vault.path(), "DROP TABLE links");
        let error = survey(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    // -- harness -------------------------------------------------------------

    /// Builds the App headlessly with the vault injected as root context —
    /// the same channel `main` uses — and returns the click targets found in
    /// the initial mutations, in document order.
    fn rendered_app(root: Option<PathBuf>) -> (VirtualDom, Vec<ElementId>) {
        let (dom, clicks, _, _) = mounted_app(root);
        (dom, clicks)
    }

    /// Like `rendered_app`, but also returns the create bar's `input` (title)
    /// and `change` (type select) listeners from the initial mutations.
    fn mounted_app(
        root: Option<PathBuf>,
    ) -> (VirtualDom, Vec<ElementId>, Vec<ElementId>, Vec<ElementId>) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        let mutations = dom.rebuild_to_vec();
        let clicks = listeners(&mutations, "click");
        let inputs = listeners(&mutations, "input");
        let changes = listeners(&mutations, "change");
        (dom, clicks, inputs, changes)
    }

    /// Mounts the App without a vault — the theme wrapper encloses the error
    /// screen too, so this is the cheapest mount — and returns the keydown
    /// target on the `.app` root.
    fn theme_app() -> (VirtualDom, ElementId) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(None)));
        let mutations = dom.rebuild_to_vec();
        let keydown = listeners(&mutations, "keydown")[0];
        (dom, keydown)
    }

    /// Fires a keydown on the app root. The physical code is irrelevant to
    /// the theme chord, which reads only the key and its modifiers.
    fn press(
        dom: &mut VirtualDom,
        target: ElementId,
        key: Key,
        modifiers: Modifiers,
    ) {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedKeyboardData::new(
                    key,
                    Code::KeyT,
                    Location::Standard,
                    false,
                    modifiers,
                    false,
                ),
            )));
            dom.runtime().handle_event(
                "keydown",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            dom.render_immediate_to_vec();
        });
    }

    /// An index built over the vault, then vandalised through a second
    /// connection — how the survey's error paths are reached.
    fn sabotaged_index(vault: &Path, sabotage: &str) -> Index {
        let notes =
            crate::index::scan_vault(vault).expect("the temp vault scans");
        let index_dir = vault.join(".index");
        std::fs::create_dir_all(&index_dir)
            .expect("the index directory is created");
        let mut index = Index::open(&index_dir.join("index.db"))
            .expect("the database opens");
        index.rebuild(&notes).expect("the rebuild succeeds");
        let saboteur = rusqlite::Connection::open(index_dir.join("index.db"))
            .expect("a second connection opens");
        saboteur
            .execute_batch(sabotage)
            .expect("the sabotage succeeds");
        index
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

    /// Fires a `change` event on the type select — the picker's only channel.
    fn change_type(dom: &mut VirtualDom, target: ElementId, value: &str) {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedFormData::new(value.to_string(), Vec::new()),
            )));
            dom.runtime().handle_event(
                "change",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            dom.render_immediate_to_vec();
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
            (
                "templates/concept.typ",
                concat!(
                    "#import \"/templates/template.typ\": *\n",
                    "#show: note\n",
                    "#meta(id: \"{{id}}\", type: \"concept\", ",
                    "created: \"{{created}}\")\n",
                    "\n= {{title}}\n",
                )
                .to_string(),
            ),
            (
                "templates/daily.typ",
                concat!(
                    "#import \"/templates/template.typ\": *\n",
                    "#show: note\n",
                    "#meta(id: \"{{id}}\", type: \"daily\", ",
                    "created: \"{{created}}\")\n",
                    "\n= {{id}}\n",
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
        // template::create writes into root/time without creating it
        std::fs::create_dir(root.join("time"))
            .expect("the time directory is created");
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

    /// Only mouse, form and keyboard events are real: the shell listens for
    /// clicks on the note list, input on the editor and keydown on the app
    /// root, so every other conversion is unreachable in these tests.
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
            event: &PlatformEventData,
        ) -> KeyboardData {
            event
                .downcast::<SerializedKeyboardData>()
                .cloned()
                .map(KeyboardData::from)
                .expect("the tests only fire serialized keyboard events")
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
