use std::cell::RefCell;
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus::prelude::*;
use jiff::civil::Date;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::blocks;
use crate::caret;
use crate::domain::{NoteCategory, NoteType};
use crate::editor::{Deletion, Editor};
use crate::index::{Index, IndexError, TableNote};
use crate::keymap;
use crate::links;
use crate::logs::{self, Selection};
use crate::loops;
use crate::palette;
use crate::positions::Positions;
use crate::render::{BodyCache, FragmentCache, RenderTheme};
use crate::table;
use crate::time;
use crate::watch;

/// One idle timer drives the save (adr/2026-07-debounced-autosave.md);
/// shortened under `cfg(test)` so the settled state is a few polls away.
#[cfg(not(test))]
const QUIET: Duration = Duration::from_millis(500);
#[cfg(test)]
const QUIET: Duration = Duration::from_millis(1);

#[derive(Clone, Debug)]
pub struct VaultRoot(pub Option<PathBuf>);

/// How Ctrl+Q reaches the windowing system: `main` injects the real
/// window-close call, the headless tests inject a recorder — the same
/// root-context channel as `VaultRoot`.
#[derive(Clone)]
pub struct Closer(pub Arc<dyn Fn() + Send + Sync>);

/// How a new card learns where the viewport's centre is: `main` injects the
/// real window's logical inner size, the headless tests inject a fixed one —
/// the `Closer` pattern; absent entirely, `table::DEFAULT_VIEWPORT` stands
/// in (adr/2026-08-new-card-lands-at-viewport-centre.md).
#[derive(Clone)]
pub struct Viewport(pub Arc<dyn Fn() -> (f64, f64) + Send + Sync>);

/// What the hit probe answers: the hit span's `data-start` byte and the
/// UTF-16 offset within its text node — or `None` off any text.
pub type Hit = Pin<Box<dyn Future<Output = Option<(usize, usize)>>>>;

/// How a mouse press finds the character it landed on: `main` injects a JS
/// `caretPositionFromPoint` walk over client coordinates, the headless
/// tests inject scripted fakes — the `Closer` pattern. This reads pointer
/// geometry, which the app never owns; the caret itself is `Editor` state
/// (adr/2026-08-caret-on-editor-note-bytes.md).
#[derive(Clone)]
pub struct HitProbe(pub Arc<dyn Fn(f64, f64) -> Hit + Send + Sync>);

/// One clipboard write, done when the future resolves.
pub type Written = Pin<Box<dyn Future<Output = ()>>>;

/// How Ctrl+C reaches the system clipboard: `main` injects a JS
/// `navigator.clipboard.writeText`, the headless tests inject a recorder —
/// the `Clipboard` seam in the other direction
/// (adr/2026-08-hidden-ime-sink.md).
#[derive(Clone)]
pub struct ClipboardWrite(pub Arc<dyn Fn(String) -> Written + Send + Sync>);

/// How the in-app capture chord reads what is on the clipboard: `main`
/// injects a JS `navigator.clipboard.readText()`, the headless tests inject
/// a scripted fake — the `CaretProbe` pattern again
/// (adr/2026-08-capture-headless-second-process.md). `None` is a clipboard
/// that would not answer, and captures nothing.
#[derive(Clone)]
pub struct Clipboard(
    #[allow(clippy::type_complexity)]
    pub  Arc<
        dyn Fn() -> Pin<Box<dyn Future<Output = Option<String>>>>
            + Send
            + Sync,
    >,
);

/// How the vault watcher reaches the screen: `main` starts the watcher on
/// its own thread and hands the receiving end over here, the headless tests
/// send batches by hand (adr/2026-08-watcher-feeds-the-ui.md). Taken out of
/// the cell once, by the shell's first render — a receiver has one owner,
/// and an app with no feed simply keeps the index it loaded at launch.
#[derive(Clone)]
pub struct VaultFeed(
    #[allow(clippy::type_complexity)]
    pub  Arc<Mutex<Option<UnboundedReceiver<Vec<watch::VaultChange>>>>>,
);

/// The clock a capture is stamped by, injected like `Today` and read only
/// when one is written (adr/2026-08-capture-timestamp-ids.md). `Today` is
/// the date every screen is drawn from and is read once at launch; a
/// capture needs the time of day too, and needs it at the moment it
/// arrives — so this is a closure rather than a value.
#[derive(Clone)]
pub struct Now(pub Arc<dyn Fn() -> jiff::Zoned + Send + Sync>);

/// The quit chord lands on the `.app` root, but the open buffer lives in
/// `Shell` — so `Shell` registers its flush here for `App` to call before
/// closing (adr/2026-07-ctrl-q-flushes-then-closes.md, reinstated by
/// adr/2026-07-hybrid-active-block-textarea.md). A plain cell, not a
/// signal: it is only ever read inside the event handler, so nothing needs
/// to re-render when it is set.
#[derive(Clone, Default)]
struct QuitFlush(Rc<RefCell<Option<Callback<(), bool>>>>);

/// Today's date, injected at the root by `main` — the app's single clock
/// edge, replaced by a fixed date in the headless tests
/// (adr/2026-07-today-injected-root-context.md).
#[derive(Clone, Copy, Debug)]
pub struct Today(pub Date);

/// The app-global commands as callbacks, provided by `App` so the Shell's
/// palette runs them through the very code paths the root chords use
/// (adr/2026-08-palette-birth-command-list.md).
#[derive(Clone, Copy)]
struct RootCommands {
    toggle_theme: Callback<()>,
    quit: Callback<()>,
}

#[component]
pub fn App() -> Element {
    let vault = use_context::<VaultRoot>();
    let today = use_context::<Today>();
    let loaded = use_hook(|| load(vault.0));
    // shared down the tree: the shell compiles note fragments in the
    // current theme's palette column
    let mut light = use_context_provider(|| Signal::new(false));
    let quit_flush = use_context_provider(QuitFlush::default);
    // absent in the headless tests, where there is no window to close
    let closer = try_consume_context::<Closer>();
    let toggle_theme = use_callback(move |()| light.set(!light()));
    let quit = use_callback(move |()| {
        // flush before close, so a quit inside the autosave's quiet window
        // cannot drop the last keystrokes; a save that fails cancels the
        // quit and surfaces its error
        // (adr/2026-07-ctrl-q-flushes-then-closes.md)
        let flush = *quit_flush.0.borrow();
        let saved = flush.is_none_or(|flush| flush.call(()));
        if saved && let Some(closer) = &closer {
            (closer.0)();
        }
    });
    use_context_provider(|| RootCommands { toggle_theme, quit });
    rsx! {
        document::Stylesheet { href: asset!("/assets/theme.css") }
        div {
            class: "app",
            // always "dark" or "light", never absent: the theme is a fact of
            // the tree, not an absence to interpret
            // (adr/2026-07-theme-attribute-on-app-root.md)
            "data-theme": if light() { "light" } else { "dark" },
            // focusable so the chords land somewhere on the vault-error
            // screen; with a vault, focus sits on the logs pane below and
            // the chords arrive here by bubbling
            tabindex: "0",
            onkeydown: move |event| {
                if event.modifiers().ctrl()
                    && event.key() == Key::Character("t".to_string())
                {
                    toggle_theme.call(());
                } else if event.modifiers().ctrl()
                    && event.key() == Key::Character("q".to_string())
                {
                    quit.call(());
                }
            },
            {
                match loaded {
                    Ok((root, notes, loops, table, edges)) => {
                        rsx! { Shell { root, notes, loops, table, edges, today: today.0 } }
                    }
                    Err(msg) => rsx! { div { class: "vault-error", "{msg}" } },
                }
            }
        }
    }
}

/// The logs screen (design § The logs screen): time rail, rendered centre
/// pane with its scale chain and "captured today" block, month-grid jump
/// panel. Everything it decides comes from `logs`; the component is wiring.
#[component]
fn Shell(
    root: PathBuf,
    notes: Vec<(String, NoteType)>,
    loops: Vec<String>,
    table: Vec<TableNote>,
    edges: Vec<(String, String)>,
    today: Date,
) -> Element {
    // the editor opens today's note before the signal takes the notes list;
    // the initializer runs once, so the launch open costs no signal write
    let mut editor = use_signal({
        let root = root.clone();
        let id = time::day_id(today);
        let exists = notes.iter().any(|(existing, _)| existing == &id);
        move || open_selected(&root, exists, &id)
    });
    let mut notes = use_signal(|| notes);
    // the open loops themselves; the ember shows how many there are and the
    // overlay shows which (adr/2026-08-loops-list-overlay.md)
    let mut loops = use_signal(|| loops);
    // the table's notes, third rider on the same survey the watcher refreshes
    let mut table_notes = use_signal(|| table);
    // the link edges, the survey's fourth rider — the constellation redraws
    // whenever the watcher redraws the cards (adr/2026-08-edges-svg-under-cards.md)
    let mut edges = use_signal(|| edges);
    // canvas positions, read once here and touched by nothing but the user's
    // drag (adr/2026-07-positions-separate-file.md)
    let mut positions = use_signal({
        let root = root.clone();
        move || Positions::load(&root.join(".index/positions"))
    });
    // which screen is up; the logs remain the door the app opens on
    let mut screen = use_signal(|| Screen::Logs);
    // the table's viewport offset, session state only — the void pans, the
    // cards keep their canvas coordinates
    let mut pan = use_signal(|| (0.0f64, 0.0f64));
    let mut grab = use_signal(|| None::<Grab>);
    // the open sheet's card id — the frozen half; where the card stands
    // re-derives from `placed` every render, which is what keeps the tether
    // on it through drags (adr/2026-08-sheet-stacking-dom-order.md)
    let mut sheet = use_signal(|| None::<String>);
    // the semantic zoom level and the observed pane size — session state
    // like the pan (adr/2026-08-body-zoom-scale-and-metrics.md,
    // adr/2026-08-viewport-culling-onresize.md)
    let mut zoom = use_signal(|| table::Zoom::Titles);
    let mut viewport = use_signal(|| table::DEFAULT_VIEWPORT);
    // the unplaced notes' session slots: a memo store like the fragment
    // cache, not UI state — nothing re-renders when a slot is remembered
    let fallback =
        use_hook(|| Rc::new(RefCell::new(table::Fallback::default())));
    let mut loops_open = use_signal(|| false);
    let mut selected = use_signal(|| (NoteType::Daily, time::day_id(today)));
    let mut month = use_signal(|| today.first_of_month());
    // the fragment cache is a memo store, not UI state: nothing should
    // re-render when it fills, so a plain hook value rather than a signal
    let fragments =
        use_hook(|| Rc::new(RefCell::new(FragmentCache::default())));
    // the body cache, its table-side sibling: per-note SVGs living until
    // the watcher invalidates them (adr/2026-08-body-cache-per-note-svg.md)
    let bodies = use_hook(|| Rc::new(RefCell::new(BodyCache::default())));
    // absent in headless tests that don't inject a fake: mouse presses then
    // land at the block's end and the clipboard chords quietly decline
    let hit = try_consume_context::<HitProbe>();
    let clipboard = try_consume_context::<Clipboard>();
    let clipboard_write = try_consume_context::<ClipboardWrite>();
    let now = try_consume_context::<Now>();
    let window_size = try_consume_context::<Viewport>();
    // always provided by App above; the palette dispatches through it
    let root_commands = use_context::<RootCommands>();

    // the link picker: open with its anchor frozen at the caret Ctrl+L
    // probed, because the textarea loses focus behind it and its text can no
    // longer move (adr/2026-08-ctrl-l-link-picker.md). The query and the
    // highlight are their own signals so every handler that moves them is
    // total — an `Option` here would branch on a state the handlers cannot
    // be in, since they only exist while the picker does.
    let mut picker = use_signal(|| None::<Picker>);
    let mut query = use_signal(String::new);
    let mut highlighted = use_signal(|| 0usize);

    // the command palette, the same split for the same reason: the frozen
    // half (what was true when Ctrl+P landed) in one signal, the moving
    // query and highlight in their own
    // (adr/2026-08-command-palette-overlay-shape.md)
    let mut palette = use_signal(|| None::<Palette>);
    let mut palette_query = use_signal(String::new);
    let mut palette_highlighted = use_signal(|| 0usize);

    // the Ctrl+N create overlay, the same split again — its frozen half also
    // remembers which step it stands in (adr/2026-08-ctrl-n-two-step-create-overlay.md);
    // the notice is overlay-local because on a bare table no editor notice
    // line is visible
    let mut creator = use_signal(|| None::<Creator>);
    let mut creator_query = use_signal(String::new);
    let mut creator_highlighted = use_signal(|| 0usize);
    let mut creator_notice = use_signal(|| None::<String>);

    // the active filter, and the Ctrl+F overlay that sets it — dims cards,
    // never drops them (adr/2026-08-filter-overlay-ctrl-f.md)
    let mut filter = use_signal(|| None::<table::Filter>);
    let mut filter_picker = use_signal(|| None::<FilterPicker>);
    let mut filter_query = use_signal(String::new);
    let mut filter_highlighted = use_signal(|| 0usize);
    // the Ctrl+O jump overlay (adr/2026-08-jump-ctrl-o-centres-viewport.md)
    let mut jump = use_signal(|| None::<Jump>);
    let mut jump_query = use_signal(String::new);
    let mut jump_highlighted = use_signal(|| 0usize);
    // the live IME composition ("^" mid–dead-key), previewed at the caret
    // and absent from the buffer until compositionend commits it
    // (adr/2026-08-hidden-ime-sink.md)
    let mut preview = use_signal(|| None::<String>);
    // a drag in flight, and whether a hit probe is already out — plain
    // cells, like QuitFlush: only the mouse handlers read them
    let dragging = use_hook(|| Rc::new(std::cell::Cell::new(false)));
    let probing = use_hook(|| Rc::new(std::cell::Cell::new(false)));

    // the Ctrl+Q flush: reports whether the open note and the canvas
    // positions reached disk, so a failed save can hold the app open
    // instead of losing either
    let quit_flush = use_callback(move |()| {
        let note_saved = editor.write().flush();
        let placed_saved = match positions.peek().save() {
            Ok(()) => true,
            Err(error) => {
                editor.write().set_notice(format!("positions: {error}"));
                false
            }
        };
        note_saved && placed_saved
    });
    let register = use_context::<QuitFlush>();
    // once is enough: the Callback's identity is stable across re-renders,
    // only its captured closure is refreshed
    use_hook(move || register.0.borrow_mut().replace(quit_flush));

    // the vault watcher, if one was handed over: every batch it debounces
    // updates the index and refreshes what the screen derives from it — the
    // rail and the open loops (adr/2026-08-watcher-feeds-the-ui.md). Taken
    // out of its cell once; a second render finds `None` and starts nothing.
    use_hook({
        let root = root.clone();
        let bodies = bodies.clone();
        move || {
            let Some(feed) = try_consume_context::<VaultFeed>() else {
                return;
            };
            let taken = feed.0.lock().ok().and_then(|mut cell| cell.take());
            let Some(mut changes) = taken else { return };
            spawn(async move {
                loop {
                    let Some(batch) = changes.recv().await else {
                        break;
                    };
                    // the body cache hears about every change first, so the
                    // repaint the survey triggers re-renders fresh bodies
                    // (adr/2026-08-body-cache-per-note-svg.md)
                    for change in &batch {
                        match change {
                            watch::VaultChange::Touched { path, .. }
                            | watch::VaultChange::Removed(path) => {
                                bodies.borrow_mut().invalidate(path);
                            }
                            watch::VaultChange::Rescan => {
                                bodies.borrow_mut().clear();
                            }
                        }
                    }
                    match refresh(&root, &batch) {
                        Ok((time_notes, open, table, links)) => {
                            notes.set(time_notes);
                            loops.set(open);
                            table_notes.set(table);
                            edges.set(links);
                        }
                        Err(message) => editor.write().set_notice(message),
                    }
                }
            });
        }
    });

    // one idle timer drives the save (adr/2026-07-debounced-autosave.md);
    // block boundaries still recompute only at the deactivation points
    let _autosave = use_resource(move || {
        // reading the editor is what subscribes this resource to every edit
        let _ = editor.read();
        async move {
            tokio::time::sleep(QUIET).await;
            let error = editor.peek().save();
            // only a value-gated write may touch the subscribed signal: an
            // unguarded one would restart this resource forever
            if let Some(error) = error
                && editor.peek().notice() != Some(error.as_str())
            {
                editor.write().set_notice(error);
            }
        }
    });

    // the drag's idle timer, the autosave's twin: every store write restarts
    // the sleep, and the file is written once the mouse rests — the restart
    // *is* the debounce (adr/2026-08-positions-plain-lines-file.md). The
    // first run after mount rewrites the just-loaded file byte-identically,
    // the autosave's same benign first tick.
    let _positions_save = use_resource(move || {
        // reading the store is what subscribes this resource to every drag
        let _ = positions.read();
        async move {
            tokio::time::sleep(QUIET).await;
            let failed = positions.peek().save().err();
            // value-gated like the autosave's: an unguarded write to the
            // subscribed editor signal would be fine, but the same message
            // re-set forever would repaint for nothing
            if let Some(error) = failed {
                let message = format!("positions: {error}");
                if editor.peek().notice() != Some(message.as_str()) {
                    editor.write().set_notice(message);
                }
            }
        }
    });

    let select = use_callback({
        let root = root.clone();
        let fragments = fragments.clone();
        move |target: Selection| {
            let anchor = logs::selection_date(&target.0, &target.1);
            // every selectable id comes from our own formatters, so the today
            // fallback guards the type system, not a reachable path
            month.set(anchor.unwrap_or(today).first_of_month());
            let exists = notes
                .peek()
                .iter()
                .any(|(existing, _)| existing == &target.1);
            editor.set(open_selected(&root, exists, &target.1));
            fragments.borrow_mut().sweep();
            selected.set(target);
        }
    });

    // the zoom change, one seam for chord and palette: the canvas point
    // under the viewport centre stays put
    // (adr/2026-08-body-zoom-scale-and-metrics.md)
    let zoom_to = use_callback(move |target: table::Zoom| {
        let current = *zoom.peek();
        if current == target {
            return;
        }
        let landed =
            table::rezoom(*pan.peek(), current, target, *viewport.peek());
        pan.set(landed);
        zoom.set(target);
    });

    // a card becomes its sheet: the note loads into the one editor
    // (adr/2026-08-sheet-reuses-the-one-editor.md), so autosave, flush and
    // the notice line keep holding untouched. The buffer reaches disk
    // before it is replaced — a failed save keeps the current note open
    // with its error rather than dropping the text. The landing half takes
    // the editor already built: creation hands the file it just wrote, where
    // the index lookup would still be a watcher debounce behind
    // (adr/2026-08-ctrl-n-two-step-create-overlay.md).
    let show_sheet = use_callback({
        let fragments = fragments.clone();
        move |(id, opened): (String, Editor)| {
            if !editor.write().flush() {
                return;
            }
            // the sheet, tether and raised card are titles-zoom constructs:
            // opening one zooms out first, one legible gesture
            // (adr/2026-08-body-zoom-scale-and-metrics.md)
            zoom_to.call(table::Zoom::Titles);
            picker.set(None);
            editor.set(opened);
            fragments.borrow_mut().sweep();
            screen.set(Screen::Table);
            sheet.set(Some(id));
        }
    });
    let open_sheet = use_callback({
        let root = root.clone();
        move |id: String| {
            if sheet.peek().as_deref() == Some(id.as_str()) {
                return;
            }
            let opened = open_sheet_note(&root, &id);
            show_sheet.call((id, opened));
        }
    });

    // escape's landing half and the screen switch's hygiene: put the card
    // back and give the one editor back to the logs' selection — the same
    // flush guard, so an unsavable sheet stays open over losing its text
    let close_sheet = use_callback({
        let root = root.clone();
        let fragments = fragments.clone();
        move |()| {
            if !editor.write().flush() {
                return;
            }
            picker.set(None);
            sheet.set(None);
            let id = selected.peek().1.clone();
            let exists =
                notes.peek().iter().any(|(existing, _)| existing == &id);
            editor.set(open_selected(&root, exists, &id));
            fragments.borrow_mut().sweep();
        }
    });

    // the sheet's delete (adr/2026-08-delete-note-palette-only-from-sheet.md):
    // unconfirmed, no trash. Deliberately not `close_sheet` — its flush
    // would rewrite the just-deleted file from the buffer. The landing half
    // is close_sheet's minus the flush: card and position go optimistically,
    // the watcher converges the index, and the dangling links the deletion
    // causes surface in the loops list as designed.
    let delete_note = use_callback({
        let root = root.clone();
        let fragments = fragments.clone();
        move |()| {
            let Some(own) = sheet.peek().clone() else {
                return;
            };
            // an error sheet holds a closed editor: nothing on disk to
            // remove, the sheet still deserves to close
            let file =
                editor.peek().note().map(|(file, _)| file.to_path_buf());
            if let Some(file) = file
                && let Err(error) = std::fs::remove_file(&file)
            {
                editor.write().set_notice(format!("delete: {error}"));
                return;
            }
            sheet.set(None);
            positions.write().remove(&own);
            table_notes.with_mut(|list| list.retain(|note| note.id != own));
            let id = selected.peek().1.clone();
            let exists =
                notes.peek().iter().any(|(existing, _)| existing == &id);
            editor.set(open_selected(&root, exists, &id));
            fragments.borrow_mut().sweep();
        }
    });

    // the create overlay's opening half — summon_palette's twin
    // (adr/2026-08-ctrl-n-two-step-create-overlay.md)
    let open_creator = use_callback(move |()| {
        creator_query.set(String::new());
        creator_highlighted.set(0);
        creator_notice.set(None);
        creator.set(Some(Creator { picked: None }));
    });

    // close_palette's twin: the focus effect hands the focus back
    let close_creator = use_callback(move |()| creator.set(None));

    // the overlay's final Enter: the note exists before the sheet opens on
    // it, and the card lands centred in the viewport with its position
    // persisted through the store's own debounce
    // (adr/2026-08-new-card-lands-at-viewport-centre.md). A refused
    // creation stays in the overlay, amendable.
    let create_note = use_callback({
        let root = root.clone();
        let window_size = window_size.clone();
        let fallback = fallback.clone();
        move |(picked, title): (NoteType, String)| {
            let created = today.to_string();
            match crate::create::permanent(&root, &picked, &title, &created) {
                Ok((id, path)) => {
                    creator.set(None);
                    let viewport = window_size
                        .as_ref()
                        .map_or(table::DEFAULT_VIEWPORT, |size| (size.0)());
                    let (x, y) = table::spawn_position(viewport, *pan.peek());
                    // a session birth slot, never a store write: the card
                    // drifts to its links as they arrive, and only a drag
                    // pins it (adr/2026-08-auto-place-strongest-link-ring.md)
                    fallback.borrow_mut().place(&id, (x, y));
                    // optimistic, the time-note idiom: the watcher batch
                    // converges the same row ~200 ms later
                    table_notes.with_mut(|list| {
                        list.push(TableNote {
                            id: id.clone(),
                            path: PathBuf::from(format!(
                                "{}/{id}.typ",
                                NoteCategory::Permanent.as_dir()
                            )),
                            kind: NoteCategory::Permanent,
                            note_type: Some(picked),
                            title: Some(title.clone()),
                            created: Some(created),
                            tags: Vec::new(),
                        });
                    });
                    show_sheet.call((id, Editor::open(path)));
                }
                Err(error) => {
                    creator_notice.set(Some(crate::create::notice(&error)));
                }
            }
        }
    });

    // the filter overlay's opening half: the tag vocabulary frozen at open,
    // the picker pattern (adr/2026-08-filter-overlay-ctrl-f.md)
    let open_filter = use_callback({
        let root = root.clone();
        move |()| match tag_names(&root) {
            Ok(tags) => {
                filter_query.set(String::new());
                filter_highlighted.set(0);
                filter_picker.set(Some(FilterPicker {
                    entries: table::filter_entries(&tags),
                }));
            }
            Err(msg) => editor.write().set_notice(msg),
        }
    });
    // closing hands focus back through the focus effect, like every overlay
    let close_filter = use_callback(move |()| filter_picker.set(None));
    let apply_filter = use_callback(move |chosen: Option<table::Filter>| {
        filter.set(chosen);
        close_filter.call(());
    });

    // the jump overlay's opening half: completions narrowed to notes with
    // cards — the table never hosts the rest
    // (adr/2026-08-jump-ctrl-o-centres-viewport.md)
    let open_jump = use_callback({
        let root = root.clone();
        move |()| match completions(&root) {
            Ok(entries) => {
                let carded: Vec<links::Completion> = entries
                    .into_iter()
                    .filter(|entry| {
                        table_notes
                            .peek()
                            .iter()
                            .any(|note| note.id == entry.id)
                    })
                    .collect();
                jump_query.set(String::new());
                jump_highlighted.set(0);
                jump.set(Some(Jump { entries: carded }));
            }
            Err(msg) => editor.write().set_notice(msg),
        }
    });
    let close_jump = use_callback(move |()| jump.set(None));
    let jump_to = use_callback({
        let fallback = fallback.clone();
        move |id: String| {
            close_jump.call(());
            // re-derived, so a fallback-slot card jumps to where it stands
            let placed = table::cards(
                &table_notes.peek(),
                &positions.peek(),
                &mut fallback.borrow_mut(),
                &edges.peek(),
                filter.peek().as_ref(),
                today,
            );
            if let Some(card) = placed.iter().find(|card| card.id == id) {
                let landed =
                    table::centre_on(card, *zoom.peek(), *viewport.peek());
                pan.set(landed);
            }
        }
    });

    // the explicit layout, palette-only: the open sheet's connected
    // component springs toward legibility for 50 clamped steps, then stops
    // — the one sanctioned mover besides the drag
    // (adr/2026-08-arrange-cluster-command.md)
    let arrange_cluster = use_callback({
        let fallback = fallback.clone();
        move |()| {
            let Some(own) = sheet.peek().clone() else {
                return;
            };
            let cluster = crate::arrange::component(&own, &edges.peek());
            let placed = table::cards(
                &table_notes.peek(),
                &positions.peek(),
                &mut fallback.borrow_mut(),
                &edges.peek(),
                filter.peek().as_ref(),
                today,
            );
            // only component members with cards arrange; dangling ids in
            // the component lay out nothing
            let seed: Vec<(String, (f64, f64))> = placed
                .iter()
                .filter(|card| cluster.contains(&card.id))
                .map(|card| (card.id.clone(), (card.x, card.y)))
                .collect();
            let ids: Vec<String> =
                seed.iter().map(|(id, _)| id.clone()).collect();
            let laid = crate::arrange::arrange(&ids, &edges.peek(), &seed);
            // one write: one repaint, one debounce restart
            positions.with_mut(|store| {
                for (id, (x, y)) in &laid {
                    store.set(id, *x, *y);
                }
            });
        }
    });

    // the small movements, lifted so chord, button, wheel and palette all
    // run one path (adr/2026-08-palette-birth-command-list.md)
    let page = use_callback(move |forward: bool| {
        month.set(logs::page_month(month(), forward));
    });
    let toggle_loops = use_callback(move |()| loops_open.set(!loops_open()));
    let go_today = use_callback(move |()| {
        select.call((NoteType::Daily, time::day_id(today)))
    });
    // the screen switch, one seam for icon, chord and palette
    // (adr/2026-08-screen-switch-gesture.md). Leaving the logs closes the
    // active block and the picker the way Escape would: their textarea and
    // input are about to unmount, and a hidden overlay waiting behind a
    // screen would reopen unasked on the way back.
    let go_table = use_callback(move |()| {
        // the block stays active behind the screen switch — the cursor
        // belongs to the note, and returning to the logs finds it again
        // (adr/2026-08-cursor-always-in-the-note.md); only the picker
        // closes, its input being about to unmount
        picker.set(None);
        screen.set(Screen::Table);
    });
    let go_logs = use_callback(move |()| {
        // the mirror hygiene: a sheet left open behind the logs would hold
        // the one editor away from the day the centre pane is showing, and
        // the table's finder overlays would reopen unasked on the way back
        if sheet.peek().is_some() {
            close_sheet.call(());
        }
        filter_picker.set(None);
        jump.set(None);
        screen.set(Screen::Logs);
    });

    // Where the logs pane is, so focus can be put back on it. A keydown
    // only bubbles up from whatever has focus, and the window's chords
    // (Ctrl+Q, Ctrl+T) are handled on the app root — so when the active
    // block's textarea unmounts, the webview drops focus on `<body>`, which
    // is *above* the app and outside every handler it has, and the chords
    // go dead until something inside is clicked. A plain cell, like
    // QuitFlush: nothing re-renders when the pane announces itself.
    let pane = use_hook(|| Rc::new(RefCell::new(None::<Rc<MountedData>>)));
    // where the invisible keyboard sink is, the pane cell's twin: the
    // widget's keystrokes and compositions land on it, so it must hold the
    // focus whenever a block is active and no overlay owns it
    // (adr/2026-08-hidden-ime-sink.md)
    let sink = use_hook(|| Rc::new(RefCell::new(None::<Rc<MountedData>>)));
    use_effect({
        let pane = pane.clone();
        let sink = sink.clone();
        move || {
            // every flag is read every time, so the effect follows them
            // all. While an overlay is up neither may take focus — the
            // overlay's input just asked for it in its own mount; when it
            // closes, this re-runs and hands the focus back
            let editing = editor.read().active().is_some();
            let listing = loops_open();
            let overlaid = palette.read().is_some()
                || creator.read().is_some()
                || picker.read().is_some()
                || filter_picker.read().is_some()
                || jump.read().is_some();
            let target = if editing && !listing {
                sink.borrow().clone()
            } else {
                pane.borrow().clone()
            };
            if let Some(handle) = target
                && !overlaid
            {
                // a headless refusal has no one to tell; the caret simply
                // stays where it was
                spawn(async move {
                    let _ = handle.set_focus(true).await;
                });
            }
        }
    });

    // one follow path for Ctrl+Enter, Ctrl+click and the palette: the
    // caret is app state now, so everyone reads the same one — no probe,
    // no frozen offsets (adr/2026-08-ctrl-enter-opens-time-links.md,
    // adr/2026-08-caret-on-editor-note-bytes.md)
    let follow_at = use_callback(move |()| {
        let target = {
            let editor = editor.peek();
            let (_, head) = editor.caret_in_block();
            editor
                .active_source()
                .and_then(|slice| links::link_at(slice, head))
        };
        let Some(target) = target else { return };
        if let Some(scale) = links::scale_of(&target, &notes.peek()) {
            // a time link followed from a sheet lands on the logs: the
            // sheet closes so the one editor is free to hold the day
            if sheet.peek().is_some() {
                close_sheet.call(());
                screen.set(Screen::Logs);
            }
            select.call((scale, target));
        } else if table_notes.peek().iter().any(|note| note.id == target) {
            // everything else the vault knows lives on the table: the link
            // opens its card's sheet — the v0 "wait for v1's table" branch
            // ends here (adr/2026-08-permanent-links-open-sheets.md)
            open_sheet.call(target);
        }
    });

    // one accept path for Enter and for a click on a row: the link lands at
    // the caret, which nothing could have moved while the picker held the
    // focus — app-owned state is frozen for free
    let accept = use_callback(move |link_id: String| {
        editor
            .write()
            .insert_at_caret(&links::format_link(&link_id));
        picker.set(None);
    });

    // the capture chord's working half, lifted so the palette runs the same
    // body; the chord's arm suppresses the webview's paste itself, so one
    // keystroke stays one action — the capture
    let capture_clipboard = use_callback({
        let root = root.clone();
        let clipboard = clipboard.clone();
        let now = now.clone();
        move |()| {
            let Some(clipboard) = clipboard.clone() else {
                return;
            };
            let Some(now) = now.clone() else { return };
            let root = root.clone();
            spawn(async move {
                let Some(pasted) = (clipboard.0)().await else {
                    return;
                };
                // one clock read stamps both halves, so a capture cannot
                // be filed on a day its id disagrees with
                let stamp = (now.0)();
                let notice = match crate::template::create_capture(
                    &root,
                    &crate::capture::capture_id(&stamp),
                    &stamp.date().to_string(),
                    &pasted,
                ) {
                    Ok(path) => {
                        format!("captured {}", crate::domain::stem_of(&path))
                    }
                    Err(err) => format!("capture: {err:?}"),
                };
                editor.write().set_notice(notice);
            });
        }
    });

    // the picker's opening half; the splice point is the caret itself,
    // which the overlay cannot move (adr/2026-08-ctrl-l-link-picker.md)
    let open_picker = use_callback({
        let root = root.clone();
        move |()| match completions(&root) {
            Ok(entries) => {
                query.set(String::new());
                highlighted.set(0);
                picker.set(Some(Picker { entries }));
            }
            Err(msg) => editor.write().set_notice(msg),
        }
    });

    // closing an overlay is just closing it: the focus effect above sees
    // the signal flip and hands the focus back to the sink or the pane,
    // and the caret never moved (adr/2026-08-caret-on-editor-note-bytes.md)
    let close_picker = use_callback(move |()| picker.set(None));
    let close_palette = use_callback(move |()| palette.set(None));

    // the palette's opening half, shared by both screens' Ctrl+P
    // (adr/2026-08-command-palette-overlay-shape.md)
    let summon_palette = use_callback(move |()| {
        palette_query.set(String::new());
        palette_highlighted.set(0);
        palette.set(Some(Palette {
            block_active: editor.peek().active().is_some(),
            on_table: *screen.peek() == Screen::Table,
            sheet_open: sheet.peek().is_some(),
            at_bodies: *zoom.peek() == table::Zoom::Bodies,
        }));
    });

    // one run path for Enter and for a click on a row. The palette closes
    // first, then the command runs; the focus effect settles whoever should
    // hold the focus, including the overlays a command opens — their inputs
    // ask again in their own mounts. Exhaustive on purpose: a CommandId
    // added without wiring does not compile
    // (adr/2026-08-palette-birth-command-list.md).
    let run_command =
        use_callback(move |(_frozen, id): (Palette, palette::CommandId)| {
            close_palette.call(());
            match id {
                palette::CommandId::ToggleTheme => {
                    root_commands.toggle_theme.call(());
                }
                palette::CommandId::Quit => root_commands.quit.call(()),
                palette::CommandId::CaptureClipboard => {
                    capture_clipboard.call(());
                }
                // the caret commands run against the caret the palette
                // opened over — app state nothing could have moved; the
                // palette lists them only over an active block, which is
                // their guard (palette.rs `available`)
                palette::CommandId::InsertLink => open_picker.call(()),
                palette::CommandId::FollowLink => follow_at.call(()),
                palette::CommandId::PreviousMonth => page.call(false),
                palette::CommandId::NextMonth => page.call(true),
                palette::CommandId::OpenLoops => toggle_loops.call(()),
                palette::CommandId::GoToToday => go_today.call(()),
                palette::CommandId::GoToTable => go_table.call(()),
                palette::CommandId::GoToLogs => go_logs.call(()),
                palette::CommandId::NewNote => open_creator.call(()),
                palette::CommandId::DeleteNote => delete_note.call(()),
                palette::CommandId::ZoomToBodies => {
                    zoom_to.call(table::Zoom::Bodies);
                }
                palette::CommandId::ZoomToTitles => {
                    zoom_to.call(table::Zoom::Titles);
                }
                palette::CommandId::FilterCards => open_filter.call(()),
                palette::CommandId::JumpToNote => open_jump.call(()),
                palette::CommandId::ArrangeCluster => {
                    arrange_cluster.call(());
                }
            }
        });

    let (scale, id) = selected();
    let note_list = notes();
    let exists = note_list.iter().any(|(existing, _)| existing == &id);
    let rows = logs::rail_rows(&note_list, Some(&(scale.clone(), id.clone())));
    let crumbs = logs::breadcrumbs(&scale, &id);
    // reading the theme here re-renders every fragment on Ctrl+T
    let light = use_context::<Signal<bool>>();
    let theme = if light() {
        RenderTheme::Light
    } else {
        RenderTheme::Dark
    };
    let notice = editor.read().notice().map(str::to_string);
    // the footer belongs to the logs' selected note; over the table the
    // editor holds the sheet's, whose backlinks the sheet counts itself
    let footer = (screen() == Screen::Logs)
        .then(|| link_footer(&root, &editor.read(), &id, &note_list))
        .flatten();

    // the sink's keystroke, translated by `keymap::action` and applied —
    // the only code that runs editor ops for typing; the v2 modal layer
    // slots between the translation and this (editor.rs, plan.md § Editor)
    let apply_action = use_callback({
        let clipboard = clipboard.clone();
        let clipboard_write = clipboard_write.clone();
        let fragments = fragments.clone();
        move |action: keymap::Action| match action {
            keymap::Action::Insert(text) => {
                editor.write().insert_at_caret(&text);
            }
            keymap::Action::NewLine => editor.write().insert_at_caret("\n"),
            keymap::Action::Backspace => {
                editor.write().delete_at_caret(Deletion::Back);
            }
            keymap::Action::Delete => {
                editor.write().delete_at_caret(Deletion::Forward);
            }
            keymap::Action::WordBackspace => {
                editor.write().delete_at_caret(Deletion::WordBack);
            }
            keymap::Action::Move { motion, select } => {
                // a vertical move on an edge line slides to the
                // neighbouring block; the fragment cache must drop the
                // block that just went from source to rendered
                let before = editor.peek().active();
                editor.write().move_caret(motion, select);
                if editor.peek().active() != before {
                    fragments.borrow_mut().sweep();
                }
            }
            keymap::Action::SelectAll => editor.write().select_all(),
            keymap::Action::Copy => {
                if let (Some(write), Some(text)) =
                    (clipboard_write.clone(), editor.peek().selected_text())
                {
                    spawn(async move { (write.0)(text).await });
                }
            }
            keymap::Action::Cut => {
                // copy, then remove the selection — which is what a
                // deletion keystroke over a selection does; without one,
                // cut is a no-op like the textarea's
                let selected = editor.peek().selected_text();
                if let (Some(write), Some(text)) =
                    (clipboard_write.clone(), selected)
                {
                    spawn(async move { (write.0)(text).await });
                    editor.write().delete_at_caret(Deletion::Back);
                }
            }
            keymap::Action::Paste => {
                // the capture seam read the other way: a clipboard that
                // will not answer pastes nothing
                if let Some(clipboard) = clipboard.clone() {
                    spawn(async move {
                        if let Some(text) = (clipboard.0)().await {
                            editor.write().insert_at_caret(&text);
                        }
                    });
                }
            }
            keymap::Action::Ignore => {}
        }
    });

    // the block panes, one closure both screens mount: the logs centre pane
    // and the writing sheet show the one editor through the one widget
    // (adr/2026-08-sheet-reuses-the-one-editor.md)
    let blocks_view = {
        let root = root.clone();
        let fragments = fragments.clone();
        let sink = sink.clone();
        let hit = hit.clone();
        let dragging = dragging.clone();
        let probing = probing.clone();
        move || -> Option<Element> {
            let panes = block_panes(
                &editor.read(),
                &root,
                theme,
                &mut fragments.borrow_mut(),
            )?;
            Some(rsx! {
                div { class: "note-blocks",
                    for pane in panes {
                        {
                            match pane {
                                Pane::Source { start, text } => {
                                    // the app draws the caret the webview
                                    // never could: the source cut into
                                    // pieces around selection and caret
                                    // (adr/2026-08-caret-on-editor-note-bytes.md)
                                    let (anchor, head) = editor.read().caret_in_block();
                                    let lines = caret::layout(
                                        &text,
                                        anchor,
                                        head,
                                        preview.read().as_deref(),
                                        caret::Shape::Bar,
                                    );
                                    rsx! {
                                        div {
                                            key: "{start}",
                                            class: "block-active",
                                            // a press asks the hit probe which character it
                                            // landed on; Ctrl makes it a follow, like
                                            // Ctrl+Enter (adr/2026-08-ctrl-enter-opens-time-links.md)
                                            onmousedown: {
                                                let hit = hit.clone();
                                                let dragging = dragging.clone();
                                                move |event: MouseEvent| {
                                                    let follow = event.modifiers().ctrl();
                                                    dragging.set(!follow);
                                                    let Some(hit) = hit.clone() else {
                                                        // headless without a fake: the caret
                                                        // holds, a Ctrl+press still follows it
                                                        if follow {
                                                            follow_at.call(());
                                                        }
                                                        return;
                                                    };
                                                    let at = event.client_coordinates();
                                                    spawn(async move {
                                                        // a miss (the empty margin) lands at
                                                        // the block's end
                                                        let (piece, units) = (hit.0)(at.x, at.y)
                                                            .await
                                                            .unwrap_or((usize::MAX, 0));
                                                        editor.write().place_in_block(piece, units, false);
                                                        if follow {
                                                            follow_at.call(());
                                                        }
                                                    });
                                                }
                                            },
                                            // the drag's moving half: one probe in flight
                                            // at a time, or a mousemove flood would queue
                                            // an eval per pixel
                                            onmousemove: {
                                                let hit = hit.clone();
                                                let dragging = dragging.clone();
                                                let probing = probing.clone();
                                                move |event: MouseEvent| {
                                                    if !dragging.get() {
                                                        return;
                                                    }
                                                    let Some(hit) = hit.clone() else { return };
                                                    if probing.replace(true) {
                                                        return;
                                                    }
                                                    let probing = probing.clone();
                                                    let at = event.client_coordinates();
                                                    spawn(async move {
                                                        let landed = (hit.0)(at.x, at.y).await;
                                                        probing.set(false);
                                                        if let Some((piece, units)) = landed {
                                                            editor.write().place_in_block(piece, units, true);
                                                        }
                                                    });
                                                }
                                            },
                                            onmouseup: {
                                                let dragging = dragging.clone();
                                                move |_| dragging.set(false)
                                            },
                                            for (row, line) in lines.into_iter().enumerate() {
                                                div { key: "{row}", class: "source-line",
                                                    for piece in line.pieces {
                                                        {
                                                            match piece.drawn() {
                                                                // the bar: keyed by position, so every move
                                                                // remounts it — restarting the blink (solid
                                                                // while typing) and the scroll-into-view
                                                                None => rsx! {
                                                                    span {
                                                                        key: "caret-{head}",
                                                                        class: "caret",
                                                                        onmounted: move |event: Event<MountedData>| async move {
                                                                            let _ = event
                                                                                .scroll_to_with_options(ScrollToOptions {
                                                                                    behavior: ScrollBehavior::Instant,
                                                                                    // nearest: a visible caret scrolls nothing
                                                                                    vertical: ScrollLogicalPosition::Nearest,
                                                                                    horizontal: ScrollLogicalPosition::Nearest,
                                                                                })
                                                                                .await;
                                                                        },
                                                                    }
                                                                },
                                                                Some((class, start, text)) => rsx! {
                                                                    span {
                                                                        key: "{start}-{class}",
                                                                        class: if !class.is_empty() { "{class}" },
                                                                        "data-start": "{start}",
                                                                        "{text}"
                                                                    }
                                                                },
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            // the invisible keyboard socket: WebKitGTK
                                            // attaches its IME only to editable elements,
                                            // so French dead keys compose here while the
                                            // app owns everything drawn
                                            // (adr/2026-08-hidden-ime-sink.md)
                                            input {
                                                class: "ime-sink",
                                                onmounted: {
                                                    let sink = sink.clone();
                                                    move |event: Event<MountedData>| {
                                                        sink.borrow_mut().replace(event.data());
                                                        async move {
                                                            let _ = event.set_focus(true).await;
                                                        }
                                                    }
                                                },
                                                onkeydown: move |event: KeyboardEvent| {
                                                    // never touch a composing keystroke: the
                                                    // IME owns it, and an open preview means
                                                    // the IME owns it whatever isComposing
                                                    // says (the spike saw both)
                                                    if event.data().is_composing()
                                                        || event.key() == Key::Dead
                                                        || preview.peek().is_some()
                                                    {
                                                        return;
                                                    }
                                                    // when the key is not ours (None), Escape and
                                                    // the app chords bubble exactly as they always
                                                    // did
                                                    if let Some(action) = keymap::action(&event.key(), event.modifiers()) {
                                                        // ours: keep the default out of the
                                                        // sink and the key off the pane
                                                        event.prevent_default();
                                                        event.stop_propagation();
                                                        apply_action.call(action);
                                                    }
                                                },
                                                oncompositionstart: move |_| {
                                                    preview.set(Some(String::new()));
                                                },
                                                oncompositionupdate: move |event: Event<CompositionData>| {
                                                    preview.set(Some(event.data().data()));
                                                },
                                                oncompositionend: move |event: Event<CompositionData>| {
                                                    // WebKitGTK can fire an empty end before
                                                    // the real one (the spike's transcript);
                                                    // only committed text lands
                                                    preview.set(None);
                                                    let committed = event.data().data();
                                                    if !committed.is_empty() {
                                                        editor.write().insert_at_caret(&committed);
                                                    }
                                                },
                                            }
                                        }
                                    }
                                }
                                Pane::Fragment { start, rendered } => rsx! {
                                    div {
                                        key: "{start}",
                                        class: "block",
                                        onclick: {
                                            let fragments = fragments.clone();
                                            move |_| {
                                                editor.write().activate(start);
                                                fragments.borrow_mut().sweep();
                                            }
                                        },
                                        {
                                            match rendered {
                                                Ok(svg) => rsx! {
                                                    div { class: "note", dangerous_inner_html: "{svg}" }
                                                },
                                                Err(msg) => rsx! {
                                                    p { class: "render-error", "{msg}" }
                                                },
                                            }
                                        }
                                    }
                                },
                            }
                        }
                    }
                }
            })
        }
    };

    // the link picker, cloned out so rsx borrows nothing from the signal it
    // also writes; one closure, because Ctrl+L answers in the sheet too
    let picker_view = move || -> Element {
        let open = picker.read().as_ref().map(|open| {
            let matches: Vec<links::Completion> =
                links::filter(&open.entries, &query.read())
                    .into_iter()
                    .cloned()
                    .collect();
            matches
        });
        match open {
            Some(matches) => {
                let rows = matches.clone();
                rsx! {
                div { class: "link-picker",
                    input {
                        class: "picker-query",
                        placeholder: "link to…",
                        onmounted: move |event| async move {
                            let _ = event.set_focus(true).await;
                        },
                        oninput: move |event| {
                            query.set(event.value());
                            highlighted.set(0);
                        },
                        onkeydown: move |event: KeyboardEvent| {
                            let key = event.key();
                            let last = matches.len().saturating_sub(1);
                            match key {
                                Key::Escape => close_picker.call(()),
                                Key::Enter => {
                                    // no matches: the keystroke does
                                    // nothing rather than guessing
                                    if let Some(entry) = matches.get(highlighted()) {
                                        accept.call(entry.id.clone());
                                    }
                                }
                                Key::ArrowDown => {
                                    highlighted.set((highlighted() + 1).min(last));
                                }
                                Key::ArrowUp => {
                                    highlighted.set(highlighted().saturating_sub(1));
                                }
                                _ => {}
                            }
                            // the picker owns every plain key while it
                            // is open; the ctrl chords still bubble
                            if !event.modifiers().ctrl() {
                                event.stop_propagation();
                            }
                        },
                    }
                    if rows.is_empty() {
                        div { class: "picker-empty", "no matching note" }
                    }
                    for (rank, entry) in rows.into_iter().enumerate() {
                        div {
                            key: "{entry.id}",
                            class: "picker-row",
                            class: if rank == highlighted() { "selected" },
                            onclick: {
                                let id = entry.id.clone();
                                move |_| accept.call(id.clone())
                            },
                            span { class: "picker-id", "{entry.id}" }
                            if let Some(title) = entry.title {
                                span { class: "picker-title", "{title}" }
                            }
                        }
                    }
                }
                }
            }
            None => rsx! {},
        }
    };
    // the palette's rows, cloned out the same way; which commands exist at
    // all was decided at open (adr/2026-08-palette-birth-command-list.md)
    let open_palette = palette().map(|frozen| {
        let matches = palette::filter(
            &palette_query.read(),
            palette::Context {
                block_active: frozen.block_active,
                on_table: frozen.on_table,
                sheet_open: frozen.sheet_open,
                at_bodies: frozen.at_bodies,
            },
        );
        (frozen, matches)
    });
    // the creator's rows, the same clone-out; only step 1 has rows to filter
    let open_creator_view = creator().map(|frozen| {
        let matches = crate::create::filter(&creator_query.read());
        let notice = creator_notice();
        (frozen, matches, notice)
    });
    let captured = (exists && scale == NoteType::Daily)
        .then(|| captured_lines(&root, &id));
    // reading the signals here is what repaints the table on a drag write,
    // a watcher batch and a filter change alike — and why an auto-placed
    // card follows its links live until a drag pins it
    let placed = table::cards(
        &table_notes.read(),
        &positions.read(),
        &mut fallback.borrow_mut(),
        &edges.read(),
        filter.read().as_ref(),
        today,
    );
    let day_ids: HashSet<&str> =
        note_list.iter().map(|(note, _)| note.as_str()).collect();
    let weeks = logs::month_grid(month());
    let seasons = logs::season_row(month());

    // the open sheet's derived pieces. One seeding closure serves a canvas
    // card and its raised copy — the same grab either way, so the raised
    // card still drags and the tether, re-derived from `placed` every
    // render, follows (adr/2026-08-sheet-stacking-dom-order.md). The card
    // markup itself stays inline in the canvas loop: its key must sit
    // directly on the loop's node for the keyed diff to hold.
    let sheet_open = sheet();
    let mut seed_grab =
        move |event: MouseEvent, id: String, x: f64, y: f64| {
            // a card grab must not also start a pan — this stop is the whole
            // card-vs-void disambiguation
            event.stop_propagation();
            let at = point(&event, zoom.peek().scale());
            grab.set(Some(Grab::Card {
                id,
                x,
                y,
                last: at,
                down: at,
            }));
        };
    let raised_layer = sheet_open
        .as_deref()
        .and_then(|open| placed.iter().find(|card| card.id == open))
        .map(|card| {
            let line = table::tether(card.x, card.y, pan());
            let seed = (card.id.clone(), card.x, card.y);
            // the raised copy is a `.table` child outside the panned
            // canvas, so its inline position carries the pan itself
            let (left, top) = (card.x + pan().0, card.y + pan().1);
            rsx! {
                div {
                    class: "card card-{card.kind.as_dir()} {card.bar} raised",
                    class: if card.dimmed { "dimmed" },
                    style: "left: {left}px; top: {top}px",
                    onmousedown: move |event: MouseEvent| {
                        seed_grab(event, seed.0.clone(), seed.1, seed.2);
                    },
                    div { class: "card-label", "{card.label}" }
                    div { class: "card-title", "{card.title}" }
                }
                // the lit edge back to where the card stands; zero width
                // when the card sits under the sheet — drawn as nothing
                div {
                    class: "tether",
                    style: "left: {line.left}px; top: {line.top}px; width: {line.width}px",
                }
            }
        });
    let sheet_footer =
        sheet_open.as_deref().map(|own| sheet_backlinks(&root, own));

    let keyboard = {
        let root = root.clone();
        let fragments = fragments.clone();
        move |event: KeyboardEvent| {
            match event.key() {
                // the open-loops list is a destination you leave; escape
                // reaches here only when no block owns it
                Key::Escape if loops_open() => loops_open.set(false),
                // in-app capture: what is on the clipboard becomes a note in
                // capture/, no required fields, nothing to fill in
                // (adr/2026-08-capture-headless-second-process.md). Shift
                // uppercases the character the browser reports, so the chord
                // is matched either way. A block is now always active over
                // an open note, so the chord captures unconditionally and
                // suppresses the webview's paste — still one action on one
                // keystroke, the capture
                // (adr/2026-08-cursor-always-in-the-note.md).
                Key::Character(ref character)
                    if character.eq_ignore_ascii_case("v")
                        && event.modifiers().ctrl()
                        && event.modifiers().shift() =>
                {
                    event.prevent_default();
                    capture_clipboard.call(());
                }
                // the link picker (adr/2026-08-ctrl-l-link-picker.md). It
                // lands here rather than on the app root because it needs the
                // editor, the caret probe and the index — none of which the
                // root has. Only ever meaningful over an active block: with
                // nothing to type into, there is no caret to anchor to.
                Key::Character(ref character)
                    if character == "l"
                        && event.modifiers().ctrl()
                        && picker.peek().is_none()
                        && editor.peek().active().is_some() =>
                {
                    open_picker.call(());
                }
                // the command palette — beside Ctrl+L for the reason its
                // ADR gives: the dispatch needs what only this pane has in
                // scope. What is true now is frozen now: by dispatch time
                // the palette's input owns the focus and the probe would
                // answer null (adr/2026-08-command-palette-overlay-shape.md)
                Key::Character(ref character)
                    if character == "p"
                        && event.modifiers().ctrl()
                        && palette.peek().is_none()
                        && picker.peek().is_none()
                        && creator.peek().is_none() =>
                {
                    // the webview answers a bare Ctrl+P with a print dialog
                    event.prevent_default();
                    summon_palette.call(());
                }
                // the create overlay
                // (adr/2026-08-ctrl-n-two-step-create-overlay.md), guarded
                // like the palette: overlays never stack
                Key::Character(ref character)
                    if character == "n"
                        && event.modifiers().ctrl()
                        && creator.peek().is_none()
                        && palette.peek().is_none()
                        && picker.peek().is_none() =>
                {
                    // the webview's own Ctrl+N would open a window
                    event.prevent_default();
                    open_creator.call(());
                }
                // the screen chords (adr/2026-08-screen-switch-gesture.md);
                // ordinals in chrome-icon order
                Key::Character(ref character)
                    if character == "1" && event.modifiers().ctrl() =>
                {
                    go_table.call(());
                }
                Key::Character(ref character)
                    if character == "2" && event.modifiers().ctrl() =>
                {
                    go_logs.call(());
                }
                // months page by keystroke as well as by scrolling; arrows
                // move the grid only, never the selection
                // (adr/2026-07-month-paging-arrow-keys.md)
                Key::ArrowLeft => page.call(false),
                Key::ArrowRight => page.call(true),
                // Ctrl+Enter follows the link under the caret
                // (adr/2026-08-ctrl-enter-opens-time-links.md). The modifier
                // is matched in the pattern so the plain-Enter arm below
                // never sees the chord — it would read it as "create the
                // selected note" and write a file the user never asked for.
                Key::Enter if event.modifiers().ctrl() => {
                    follow_at.call(());
                }
                // only enter writes the file — navigating never does
                Key::Enter => {
                    let (scale, id) = selected();
                    if notes.read().iter().any(|(existing, _)| existing == &id)
                    {
                        return;
                    }
                    let created =
                        logs::selection_date(&scale, &id).unwrap_or(today);
                    match crate::template::create(
                        &root,
                        &NoteCategory::Time,
                        &scale,
                        &id,
                        &created.to_string(),
                        "",
                    ) {
                        Ok(_) => {
                            editor
                                .set(Editor::open(time_note_path(&root, &id)));
                            fragments.borrow_mut().sweep();
                            notes.with_mut(|list| list.push((id, scale)));
                        }
                        Err(err) => editor
                            .write()
                            .set_notice(format!("create: {err:?}")),
                    }
                }
                _ => {}
            }
        }
    };

    // the table pane's own chords: the palette, the screens, and — now that
    // the sheet holds the editor here — the editor chords the logs pane has;
    // everything else bubbles to the app root
    let table_keys = {
        move |event: KeyboardEvent| match event.key() {
            // escape reaches here only when no block or overlay owns it:
            // the sheet closes and the card goes back (wireframe 6b)
            Key::Escape if sheet.peek().is_some() => close_sheet.call(()),
            // the link picker over the sheet's active block — the logs
            // pane's arm, guard for guard (adr/2026-08-ctrl-l-link-picker.md)
            Key::Character(ref character)
                if character == "l"
                    && event.modifiers().ctrl()
                    && picker.peek().is_none()
                    && editor.peek().active().is_some() =>
            {
                open_picker.call(());
            }
            Key::Character(ref character)
                if character == "p"
                    && event.modifiers().ctrl()
                    && palette.peek().is_none()
                    && picker.peek().is_none()
                    && creator.peek().is_none()
                    && filter_picker.peek().is_none()
                    && jump.peek().is_none() =>
            {
                // the webview answers a bare Ctrl+P with a print dialog
                event.prevent_default();
                summon_palette.call(());
            }
            // the create overlay, the logs arm's twin
            // (adr/2026-08-ctrl-n-two-step-create-overlay.md)
            Key::Character(ref character)
                if character == "n"
                    && event.modifiers().ctrl()
                    && creator.peek().is_none()
                    && palette.peek().is_none()
                    && picker.peek().is_none()
                    && filter_picker.peek().is_none()
                    && jump.peek().is_none() =>
            {
                // the webview's own Ctrl+N would open a window
                event.prevent_default();
                open_creator.call(());
            }
            Key::Character(ref character)
                if character == "1" && event.modifiers().ctrl() =>
            {
                go_table.call(());
            }
            Key::Character(ref character)
                if character == "2" && event.modifiers().ctrl() =>
            {
                go_logs.call(());
            }
            // Ctrl+Enter follows the link under the caret, sheet to sheet
            // or sheet to logs (adr/2026-08-permanent-links-open-sheets.md)
            Key::Enter if event.modifiers().ctrl() => {
                follow_at.call(());
            }
            // the semantic zoom pair, table-only
            // (adr/2026-08-body-zoom-scale-and-metrics.md)
            Key::Character(ref character)
                if character == "=" && event.modifiers().ctrl() =>
            {
                // the webview owns Ctrl+= as page zoom
                event.prevent_default();
                zoom_to.call(table::Zoom::Bodies);
            }
            Key::Character(ref character)
                if character == "-" && event.modifiers().ctrl() =>
            {
                event.prevent_default();
                zoom_to.call(table::Zoom::Titles);
            }
            // the finders, table-only, guarded like every overlay chord
            // (adr/2026-08-filter-overlay-ctrl-f.md,
            // adr/2026-08-jump-ctrl-o-centres-viewport.md)
            Key::Character(ref character)
                if character == "f"
                    && event.modifiers().ctrl()
                    && filter_picker.peek().is_none()
                    && jump.peek().is_none()
                    && palette.peek().is_none()
                    && creator.peek().is_none()
                    && picker.peek().is_none() =>
            {
                // the webview owns Ctrl+F as find-in-page
                event.prevent_default();
                open_filter.call(());
            }
            Key::Character(ref character)
                if character == "o"
                    && event.modifiers().ctrl()
                    && jump.peek().is_none()
                    && filter_picker.peek().is_none()
                    && palette.peek().is_none()
                    && creator.peek().is_none()
                    && picker.peek().is_none() =>
            {
                // the webview owns Ctrl+O as an open dialog
                event.prevent_default();
                open_jump.call(());
            }
            _ => {}
        }
    };

    rsx! {
        Chrome {
            screen: screen(),
            loops: loops.read().len(),
            filter: filter.read().as_ref().map(table::filter_label),
            on_ember: move |_| toggle_loops.call(()),
            on_table: move |_| go_table.call(()),
            on_logs: move |_| go_logs.call(()),
        }
        // the palette floats (position: fixed) over whichever screen is
        // up, so it lives beside the panes rather than inside one
        {
            match open_palette {
                Some((frozen, matches)) => {
                    let rows = matches.clone();
                    rsx! {
                    div { class: "command-palette",
                        div { class: "palette-head type-label", "commands" }
                        input {
                            class: "picker-query",
                            placeholder: "command…",
                            onmounted: move |event| async move {
                                let _ = event.set_focus(true).await;
                            },
                            oninput: move |event| {
                                palette_query.set(event.value());
                                palette_highlighted.set(0);
                            },
                            onkeydown: move |event: KeyboardEvent| {
                                let key = event.key();
                                let last = matches.len().saturating_sub(1);
                                match key {
                                    Key::Escape => close_palette.call(()),
                                    Key::Enter => {
                                        // no matches: the keystroke does
                                        // nothing rather than guessing
                                        if let Some(command) = matches.get(palette_highlighted()) {
                                            run_command.call((frozen, command.id));
                                        }
                                    }
                                    Key::ArrowDown => {
                                        palette_highlighted.set((palette_highlighted() + 1).min(last));
                                    }
                                    Key::ArrowUp => {
                                        palette_highlighted.set(palette_highlighted().saturating_sub(1));
                                    }
                                    _ => {}
                                }
                                // the palette owns every plain key while
                                // it is open; the ctrl chords still bubble
                                if !event.modifiers().ctrl() {
                                    event.stop_propagation();
                                }
                            },
                        }
                        if rows.is_empty() {
                            div { class: "picker-empty", "no matching command" }
                        }
                        for (rank, command) in rows.into_iter().enumerate() {
                            div {
                                key: "{command.label}",
                                class: "palette-row",
                                class: if rank == palette_highlighted() { "selected" },
                                onclick: {
                                    let id = command.id;
                                    move |_| run_command.call((frozen, id))
                                },
                                span { class: "palette-label", "{command.label}" }
                                if let Some(chord) = command.chord {
                                    span { class: "palette-chord", "{chord}" }
                                }
                            }
                        }
                    }
                    }
                }
                None => rsx! {},
            }
        }
        // the create overlay floats in the palette's exact box — the same
        // grammar, one more step (adr/2026-08-ctrl-n-two-step-create-overlay.md)
        {
            match open_creator_view {
                Some((frozen, matches, notice)) => {
                    let head = frozen.picked.as_ref().map_or_else(
                        || "new note".to_string(),
                        |picked| format!("new {}", picked.as_name()),
                    );
                    let step_two = frozen.picked.is_some();
                    let hint = if step_two { "title…" } else { "type…" };
                    let rows = if step_two { Vec::new() } else { matches.clone() };
                    let keydown_frozen = frozen.clone();
                    rsx! {
                    div { class: "command-palette",
                        div { class: "palette-head type-label", "{head}" }
                        input {
                            // controlled, unlike the palette's: the step
                            // transition clears the query signal, and the
                            // input must follow it — text left behind would
                            // become a title no one typed
                            value: "{creator_query}",
                            class: "picker-query",
                            placeholder: "{hint}",
                            onmounted: move |event| async move {
                                let _ = event.set_focus(true).await;
                            },
                            oninput: move |event| {
                                creator_query.set(event.value());
                                creator_highlighted.set(0);
                                creator_notice.set(None);
                            },
                            onkeydown: move |event: KeyboardEvent| {
                                let key = event.key();
                                let last = matches.len().saturating_sub(1);
                                match key {
                                    // escape backs out step by step:
                                    // title → type list → closed
                                    Key::Escape => {
                                        if keydown_frozen.picked.is_some() {
                                            creator_query.set(String::new());
                                            creator_highlighted.set(0);
                                            creator_notice.set(None);
                                            creator.set(Some(Creator {
                                                picked: None,
                                            }));
                                        } else {
                                            close_creator.call(());
                                        }
                                    }
                                    Key::Enter => match &keydown_frozen.picked {
                                        // step 2: the input is the title
                                        Some(picked) => {
                                            create_note.call((
                                                picked.clone(),
                                                creator_query.peek().clone(),
                                            ));
                                        }
                                        // step 1: no matches, no guess
                                        None => {
                                            if let Some(picked) = matches.get(creator_highlighted()) {
                                                creator_query.set(String::new());
                                                creator_highlighted.set(0);
                                                creator.set(Some(Creator {
                                                    picked: Some(picked.clone()),
                                                }));
                                            }
                                        }
                                    },
                                    Key::ArrowDown => {
                                        creator_highlighted.set((creator_highlighted() + 1).min(last));
                                    }
                                    Key::ArrowUp => {
                                        creator_highlighted.set(creator_highlighted().saturating_sub(1));
                                    }
                                    _ => {}
                                }
                                // the overlay owns every plain key while it
                                // is open; the ctrl chords still bubble
                                if !event.modifiers().ctrl() {
                                    event.stop_propagation();
                                }
                            },
                        }
                        if let Some(message) = notice {
                            p { class: "render-error", "{message}" }
                        }
                        if !step_two && rows.is_empty() {
                            div { class: "picker-empty", "no matching type" }
                        }
                        for (rank, entry) in rows.into_iter().enumerate() {
                            div {
                                key: "{entry.as_name()}",
                                class: "picker-row",
                                class: if rank == creator_highlighted() { "selected" },
                                onclick: {
                                    let picked = entry.clone();
                                    move |_| {
                                        creator_query.set(String::new());
                                        creator_highlighted.set(0);
                                        creator.set(Some(Creator {
                                            picked: Some(picked.clone()),
                                        }));
                                    }
                                },
                                span { class: "picker-id", "{entry.as_name()}" }
                            }
                        }
                    }
                    }
                }
                None => rsx! {},
            }
        }
        if screen() == Screen::Logs {
            div {
                class: "logs",
                // the enter-to-create keystroke lands here and the theme/quit
                // chords bubble on up to the .app root — which is why the pane
                // takes focus back whenever no block holds it
                tabindex: "0",
                autofocus: true,
                onmounted: {
                    let pane = pane.clone();
                    move |event: Event<MountedData>| {
                        pane.borrow_mut().replace(event.data());
                        // a return from the table must land focus here
                        // itself: autofocus fired at document load only, and
                        // the table pane just unmounted the focus with it
                        let handle = event.data();
                        async move {
                            let _ = handle.set_focus(true).await;
                        }
                    }
                },
                onkeydown: keyboard,
                nav { class: "rail",
                    for row in rows {
                        div {
                            key: "{row.id}",
                            class: "rail-row rail-{row.scale.as_name()}",
                            class: if row.id == id { "selected" },
                            class: if !row.exists { "missing" },
                            onclick: {
                                let target = (row.scale.clone(), row.id.clone());
                                move |_| select.call(target.clone())
                            },
                            span { class: "rail-id", "{row.id}" }
                            if !logs::rail_tag(&row.scale).is_empty() {
                                span { class: "rail-tag", "{logs::rail_tag(&row.scale)}" }
                            }
                        }
                    }
                }
                section { class: "centre",
                    div { class: "crumbs",
                        for crumb in crumbs {
                            {
                                match crumb.target {
                                    Some(target) => rsx! {
                                        span {
                                            class: "crumb crumb-link",
                                            onclick: move |_| select.call(target.clone()),
                                            "{crumb.label}"
                                        }
                                    },
                                    None => rsx! {
                                        span { class: "crumb", "{crumb.label}" }
                                    },
                                }
                            }
                        }
                    }
                    {
                        match &notice {
                            Some(msg) => rsx! { p { class: "render-error", "{msg}" } },
                            None => rsx! {},
                        }
                    }
                    {
                        match blocks_view() {
                            Some(view) => view,
                            // the note exists but would not open: the notice
                            // above carries the error, the pane stays bare
                            None if exists => rsx! {},
                            // empty is honest: no ghost template, one line
                            None => rsx! {
                                p { class: "empty-note",
                                    "no note for {logs::selection_label(&scale, &id)} — press "
                                    kbd { "enter" }
                                    " to start one from the template"
                                }
                            },
                        }
                    }
                    {picker_view()}
                    // the ember's destination: what the count is made of, and
                    // nothing else — no ages, no grouping, no per-item actions
                    // (adr/2026-07-debt-counter-then-list.md)
                    if loops_open() && !loops.read().is_empty() {
                        div { class: "loops-list",
                            div { class: "loops-head type-label", "open loops" }
                            for line in loops() {
                                div { key: "{line}", class: "loops-line", "{line}" }
                            }
                        }
                    }
                    {
                        match footer {
                            Some(Ok((back, out))) if !back.is_empty() || !out.is_empty() => rsx! {
                                div { class: "links-footer",
                                    for (arrow, entries) in [("←", back), ("→", out)] {
                                        if !entries.is_empty() {
                                            div { class: "links-row",
                                                span { class: "links-arrow", "{arrow}" }
                                                for link in entries {
                                                    {
                                                        match link.scale {
                                                            // a time note opens in the
                                                            // centre pane it stands over
                                                            Some(scale) => {
                                                                let target = (scale, link.label.clone());
                                                                rsx! {
                                                                    span {
                                                                        class: "link-entry link-jump",
                                                                        onclick: move |_| select.call(target.clone()),
                                                                        "{link.label}"
                                                                    }
                                                                }
                                                            }
                                                            // the rest opens its card's sheet
                                                            // (adr/2026-08-permanent-links-open-sheets.md);
                                                            // dangling — or labelled by a stem
                                                            // the table cannot host — stays inert
                                                            None => {
                                                                let on_table = !link.dangling
                                                                    && table_notes
                                                                        .read()
                                                                        .iter()
                                                                        .any(|note| note.id == link.label);
                                                                if on_table {
                                                                    let target = link.label.clone();
                                                                    rsx! {
                                                                        span {
                                                                            class: "link-entry link-jump",
                                                                            onclick: move |_| open_sheet.call(target.clone()),
                                                                            "{link.label}"
                                                                        }
                                                                    }
                                                                } else {
                                                                    rsx! {
                                                                        span {
                                                                            class: "link-entry",
                                                                            class: if link.dangling { "link-dangling" },
                                                                            "{link.label}"
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            Some(Err(msg)) => rsx! { p { class: "render-error", "{msg}" } },
                            _ => rsx! {},
                        }
                    }
                    {
                        match captured {
                            Some(Ok(lines)) if !lines.is_empty() => rsx! {
                                div { class: "captured",
                                    div { class: "captured-head type-label", "captured today" }
                                    for line in lines {
                                        div { class: "captured-line", "{line}" }
                                    }
                                }
                            },
                            Some(Err(msg)) => rsx! {
                                p { class: "render-error", "{msg}" }
                            },
                            _ => rsx! {},
                        }
                    }
                }
                aside {
                    class: "jump",
                    // months page by scrolling — no ‹ › buttons (design § logs)
                    onwheel: move |event| {
                        let delta = event.delta().strip_units().y;
                        if delta != 0.0 {
                            page.call(delta > 0.0);
                        }
                    },
                    div { class: "cal-head",
                        span { class: "cal-month type-label", "{logs::month_label(month())}" }
                        // ‹ today › — the mockup's header controls
                        // (adr/2026-07-month-paging-arrow-keys.md)
                        span { class: "cal-nav",
                            button {
                                class: "cal-arrow",
                                onclick: move |_| page.call(false),
                                "‹"
                            }
                            button {
                                class: "cal-today",
                                onclick: move |_| go_today.call(()),
                                "today"
                            }
                            button {
                                class: "cal-arrow",
                                onclick: move |_| page.call(true),
                                "›"
                            }
                        }
                    }
                    div { class: "cal-grid",
                        span { class: "cal-gutter" }
                        for letter in ["m", "t", "w", "t", "f", "s", "s"] {
                            span { class: "cal-weekday", "{letter}" }
                        }
                        for week in weeks {
                            span {
                                class: "cal-gutter cal-week",
                                class: if week.week_id == id { "selected" },
                                onclick: {
                                    let target = week.week_id.clone();
                                    move |_| select.call((NoteType::Weekly, target.clone()))
                                },
                                "{week.label}"
                            }
                            for cell in week.days {
                                {
                                    match cell {
                                        Some(day) => {
                                            let cell_id = time::day_id(day);
                                            let has_note = day_ids.contains(cell_id.as_str());
                                            rsx! {
                                                span {
                                                    class: "cal-day",
                                                    class: if has_note { "has-note" },
                                                    // selection ≠ existence: a
                                                    // selected empty day outlines
                                                    class: if cell_id == id { "selected" },
                                                    onclick: move |_| {
                                                        select.call((
                                                            NoteType::Daily,
                                                            cell_id.clone(),
                                                        ))
                                                    },
                                                    "{day.day()}"
                                                }
                                            }
                                        }
                                        None => rsx! { span { class: "cal-day blank" } },
                                    }
                                }
                            }
                        }
                    }
                    div { class: "cal-seasons",
                        for (label, target) in seasons {
                            span {
                                class: "cal-season",
                                class: if target.1 == time::season_id(today) { "lit" },
                                class: if target.1 == id { "selected" },
                                onclick: move |_| select.call(target.clone()),
                                "{label}"
                            }
                        }
                    }
                }
            }
        }
        if screen() == Screen::Table {
            div {
                class: "table",
                // switching here unmounted the logs pane and the focus it
                // held; the chords only arrive by bubbling from inside, so
                // the pane must ask for focus itself — autofocus fires at
                // document load only (the textarea's onmounted lesson)
                tabindex: "0",
                onmounted: {
                    let pane = pane.clone();
                    move |event: Event<MountedData>| {
                        pane.borrow_mut().replace(event.data());
                        let handle = event.data();
                        async move {
                            let _ = handle.set_focus(true).await;
                        }
                    }
                },
                onkeydown: table_keys,
                // the pane's observed size feeds the culling; the observer
                // fires immediately on mount and on every resize — a
                // refusal keeps the deterministic default
                // (adr/2026-08-viewport-culling-onresize.md)
                onresize: move |event: Event<ResizeData>| {
                    if let Ok(size) = event.get_border_box_size() {
                        viewport.set((size.width, size.height));
                    }
                },
                // a mousedown that no card stopped is the void: pan
                onmousedown: move |event: MouseEvent| {
                    grab.set(Some(Grab::Void {
                        last: point(&event, zoom.peek().scale()),
                    }));
                },
                // one total handler moves whatever is held; `.peek()`
                // everywhere, so tracking the mouse subscribes nothing
                onmousemove: move |event: MouseEvent| {
                    // cloned out first: the peek guard must drop before the
                    // arms write the signal back
                    let held = grab.peek().clone();
                    match held {
                        None => {}
                        Some(Grab::Void { last }) => {
                            let now = point(&event, zoom.peek().scale());
                            let (x, y) = *pan.peek();
                            pan.set((x + now.0 - last.0, y + now.1 - last.1));
                            grab.set(Some(Grab::Void { last: now }));
                        }
                        Some(Grab::Card { id, x, y, last, down }) => {
                            let now = point(&event, zoom.peek().scale());
                            let (x, y) = (x + now.0 - last.0, y + now.1 - last.1);
                            // the live repaint and the debounce restart are
                            // the same write
                            positions.write().set(&id, x, y);
                            grab.set(Some(Grab::Card { id, x, y, last: now, down }));
                        }
                    }
                },
                // the whole travel decides at release: within the slop the
                // press was a click and the card opens its sheet — a
                // sub-slop wobble still wrote its honest pixel or two
                // (adr/2026-08-click-opens-drag-moves.md)
                onmouseup: move |event: MouseEvent| {
                    let held = grab.peek().clone();
                    grab.set(None);
                    if let Some(Grab::Card { id, down, .. }) = held
                        && table::is_click(
                            down,
                            point(&event, zoom.peek().scale()),
                        )
                    {
                        open_sheet.call(id);
                    }
                },
                div {
                    class: "canvas",
                    // scale outermost: the pan stays in canvas units, and
                    // point() divides once
                    // (adr/2026-08-body-zoom-scale-and-metrics.md)
                    style: "transform: scale({zoom().scale()}) translate({pan().0}px, {pan().1}px)",
                    // the constellation: first child, so DOM order paints
                    // every edge under every card; inside the translated
                    // canvas, so pan and drags carry it for free
                    // (adr/2026-08-edges-svg-under-cards.md)
                    svg { class: "edges",
                        for edge in table::edges(&edges.read(), &placed) {
                            g {
                                key: "{edge.source}-{edge.target}",
                                line {
                                    x1: "{edge.x1}",
                                    y1: "{edge.y1}",
                                    x2: "{edge.x2}",
                                    y2: "{edge.y2}",
                                }
                                circle { cx: "{edge.x1}", cy: "{edge.y1}", r: "2" }
                                circle { cx: "{edge.x2}", cy: "{edge.y2}", r: "2" }
                            }
                        }
                    }
                    // the origin card leaves the canvas while its sheet is
                    // open — it re-renders raised above the dim instead;
                    // off-viewport cards render nothing at all — the "low
                    // thousands" answer (adr/2026-08-viewport-culling-onresize.md)
                    for card in placed
                        .iter()
                        .filter(|card| sheet_open.as_deref() != Some(card.id.as_str()))
                        .filter(|card| table::in_view(card, zoom(), pan(), viewport()))
                    {
                        div {
                            key: "{card.id}",
                            class: "card card-{card.kind.as_dir()} {card.bar}",
                            class: if zoom() == table::Zoom::Bodies { "bodies" },
                            class: if card.dimmed { "dimmed" },
                            style: "left: {card.x}px; top: {card.y}px",
                            onmousedown: {
                                let seed = (card.id.clone(), card.x, card.y);
                                move |event: MouseEvent| {
                                    seed_grab(event, seed.0.clone(), seed.1, seed.2);
                                }
                            },
                            div { class: "card-label", "{card.label}" }
                            div { class: "card-title", "{card.title}" }
                            // the note's own rendered body, clipped — the
                            // template's typography, never restyled
                            // (adr/2026-08-body-cache-per-note-svg.md)
                            if zoom() == table::Zoom::Bodies {
                                div { class: "card-body",
                                    {
                                        match bodies.borrow_mut().render(&root, &card.path, theme) {
                                            Ok(svg) => rsx! {
                                                div { class: "note", dangerous_inner_html: "{svg}" }
                                            },
                                            Err(msg) => rsx! {
                                                p { class: "render-error", "{msg}" }
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if sheet_open.is_some() {
                    // wireframe state 6b, painted in DOM order rather than
                    // z-index: dim over the canvas, the origin card and its
                    // tether over the dim, the sheet over everything
                    // (adr/2026-08-sheet-stacking-dom-order.md). The dim has
                    // no handlers — its presses fall through to the pane, so
                    // the table still pans under the sheet.
                    div { class: "dim" }
                    {raised_layer.unwrap_or_else(|| rsx! {})}
                    aside {
                        class: "sheet",
                        style: "left: {table::SHEET_LEFT}px; width: {table::SHEET_WIDTH}px",
                        // a press inside the sheet is the sheet's own (text
                        // selection, block clicks) — never the void's pan
                        onmousedown: move |event: MouseEvent| event.stop_propagation(),
                        {
                            match &notice {
                                Some(msg) => rsx! { p { class: "render-error", "{msg}" } },
                                None => rsx! {},
                            }
                        }
                        {blocks_view().unwrap_or_else(|| rsx! {})}
                        {picker_view()}
                        {
                            // backlinks only, as a count ("← 2") — absent at
                            // zero, the ember's idiom (design § Chrome)
                            match sheet_footer {
                                Some(Ok(count)) if count > 0 => rsx! {
                                    div { class: "sheet-footer", "← {count}" }
                                },
                                Some(Err(msg)) => rsx! { p { class: "render-error", "{msg}" } },
                                _ => rsx! {},
                            }
                        }
                    }
                }
                // the finders float in the palette's box, but live inside
                // the table branch: the screen switch unmounts them and
                // go_logs clears their state
                // (adr/2026-08-filter-overlay-ctrl-f.md)
                {
                    match filter_picker() {
                        Some(frozen) => {
                            let rows: Vec<table::FilterEntry> =
                                table::filter_rows(&filter_query.read(), &frozen.entries)
                                    .into_iter()
                                    .cloned()
                                    .collect();
                            let keys_rows = rows.clone();
                            rsx! {
                            div { class: "command-palette",
                                div { class: "palette-head type-label", "filter" }
                                input {
                                    class: "picker-query",
                                    placeholder: "tag or type…",
                                    onmounted: move |event| async move {
                                        let _ = event.set_focus(true).await;
                                    },
                                    oninput: move |event| {
                                        filter_query.set(event.value());
                                        filter_highlighted.set(0);
                                    },
                                    onkeydown: move |event: KeyboardEvent| {
                                        let key = event.key();
                                        let last = keys_rows.len().saturating_sub(1);
                                        match key {
                                            Key::Escape => close_filter.call(()),
                                            Key::Enter => {
                                                // an empty query clears the
                                                // active filter — the
                                                // re-summon-and-clear gesture
                                                if filter_query.peek().is_empty() {
                                                    apply_filter.call(None);
                                                } else if let Some(entry) = keys_rows.get(filter_highlighted()) {
                                                    apply_filter.call(Some(entry.filter.clone()));
                                                }
                                            }
                                            Key::ArrowDown => {
                                                filter_highlighted.set((filter_highlighted() + 1).min(last));
                                            }
                                            Key::ArrowUp => {
                                                filter_highlighted.set(filter_highlighted().saturating_sub(1));
                                            }
                                            _ => {}
                                        }
                                        if !event.modifiers().ctrl() {
                                            event.stop_propagation();
                                        }
                                    },
                                }
                                if rows.is_empty() {
                                    div { class: "picker-empty", "no matching filter" }
                                }
                                for (rank, entry) in rows.into_iter().enumerate() {
                                    {
                                        let kind = match entry.filter {
                                            table::Filter::Tag(_) => "tag",
                                            table::Filter::Type(_) => "type",
                                        };
                                        rsx! {
                                            div {
                                                key: "{kind}-{entry.label}",
                                                class: "picker-row",
                                                class: if rank == filter_highlighted() { "selected" },
                                                onclick: {
                                                    let chosen = entry.filter.clone();
                                                    move |_| apply_filter.call(Some(chosen.clone()))
                                                },
                                                span { class: "picker-id", "{entry.label}" }
                                                span { class: "picker-title", "{kind}" }
                                            }
                                        }
                                    }
                                }
                            }
                            }
                        }
                        None => rsx! {},
                    }
                }
                {
                    match jump() {
                        Some(frozen) => {
                            let rows: Vec<links::Completion> =
                                links::filter(&frozen.entries, &jump_query.read())
                                    .into_iter()
                                    .cloned()
                                    .collect();
                            let keys_rows = rows.clone();
                            rsx! {
                            div { class: "command-palette",
                                div { class: "palette-head type-label", "jump" }
                                input {
                                    class: "picker-query",
                                    placeholder: "note…",
                                    onmounted: move |event| async move {
                                        let _ = event.set_focus(true).await;
                                    },
                                    oninput: move |event| {
                                        jump_query.set(event.value());
                                        jump_highlighted.set(0);
                                    },
                                    onkeydown: move |event: KeyboardEvent| {
                                        let key = event.key();
                                        let last = keys_rows.len().saturating_sub(1);
                                        match key {
                                            Key::Escape => close_jump.call(()),
                                            Key::Enter => {
                                                if let Some(entry) = keys_rows.get(jump_highlighted()) {
                                                    jump_to.call(entry.id.clone());
                                                }
                                            }
                                            Key::ArrowDown => {
                                                jump_highlighted.set((jump_highlighted() + 1).min(last));
                                            }
                                            Key::ArrowUp => {
                                                jump_highlighted.set(jump_highlighted().saturating_sub(1));
                                            }
                                            _ => {}
                                        }
                                        if !event.modifiers().ctrl() {
                                            event.stop_propagation();
                                        }
                                    },
                                }
                                if rows.is_empty() {
                                    div { class: "picker-empty", "no matching note" }
                                }
                                for (rank, entry) in rows.into_iter().enumerate() {
                                    div {
                                        key: "{entry.id}",
                                        class: "picker-row",
                                        class: if rank == jump_highlighted() { "selected" },
                                        onclick: {
                                            let id = entry.id.clone();
                                            move |_| jump_to.call(id.clone())
                                        },
                                        span { class: "picker-id", "{entry.id}" }
                                        if let Some(title) = entry.title {
                                            span { class: "picker-title", "{title}" }
                                        }
                                    }
                                }
                            }
                            }
                        }
                        None => rsx! {},
                    }
                }
            }
        }
    }
}

/// The app's two screens (adr/2026-07-two-screens-table-and-logs.md): the
/// table mounts as of v1 phase 2.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Table,
    Logs,
}

/// What the mouse holds on the table: the void (panning) or a card (moving
/// it). `last` is the previous mousemove in client coordinates; the card's
/// `x, y` are its canvas coordinates, authoritative while the drag lasts —
/// seeded from the render, so a click that never moves writes nothing.
/// `down` is where the press landed, never mutated: mouseup measures the
/// whole travel against it to tell a click from a drag
/// (adr/2026-08-click-opens-drag-moves.md).
#[derive(Clone, PartialEq)]
enum Grab {
    Void {
        last: (f64, f64),
    },
    Card {
        id: String,
        x: f64,
        y: f64,
        last: (f64, f64),
        down: (f64, f64),
    },
}

/// Canvas-unit coordinates: client divided by the zoom's scale — the
/// division phase 2 promised would land here and nowhere else. The scale
/// sits outside the translate, so pan and drag deltas both live in canvas
/// units and move 1:1 on screen
/// (adr/2026-08-body-zoom-scale-and-metrics.md).
fn point(event: &MouseEvent, scale: f64) -> (f64, f64) {
    let coordinates = event.client_coordinates();
    (coordinates.x / scale, coordinates.y / scale)
}

/// The one-line chrome (design § Chrome): two 14×14 stroked icons, the
/// current screen's lit and each a button to its screen
/// (adr/2026-08-screen-switch-gesture.md), and the open-loops ember. Zero
/// loops renders nothing at all — absence, not a zero — so the ember is
/// clickable exactly when there is a list to show
/// (adr/2026-08-loops-list-overlay.md).
#[component]
fn Chrome(
    screen: Screen,
    loops: usize,
    filter: Option<String>,
    on_ember: EventHandler<()>,
    on_table: EventHandler<()>,
    on_logs: EventHandler<()>,
) -> Element {
    rsx! {
        header { class: "chrome",
            svg {
                class: if screen == Screen::Table { "icon-table lit" } else { "icon-table" },
                width: "14",
                height: "14",
                view_box: "0 0 14 14",
                onclick: move |_| on_table.call(()),
                rect { x: "1", y: "2", width: "5", height: "4", fill: "none", stroke: "currentColor" }
                rect { x: "8", y: "5", width: "5", height: "4", fill: "none", stroke: "currentColor" }
                rect { x: "3", y: "9", width: "5", height: "4", fill: "none", stroke: "currentColor" }
            }
            svg {
                class: if screen == Screen::Logs { "icon-logs lit" } else { "icon-logs" },
                width: "14",
                height: "14",
                view_box: "0 0 14 14",
                onclick: move |_| on_logs.call(()),
                rect { x: "1.5", y: "2.5", width: "11", height: "10", fill: "none", stroke: "currentColor" }
                line { x1: "1.5", y1: "5.5", x2: "12.5", y2: "5.5", stroke: "currentColor" }
                line { x1: "4.5", y1: "1", x2: "4.5", y2: "3.5", stroke: "currentColor" }
                line { x1: "9.5", y1: "1", x2: "9.5", y2: "3.5", stroke: "currentColor" }
            }
            // the active filter, named: the map reads differently under
            // one, and the chrome must say why
            // (adr/2026-08-filter-overlay-ctrl-f.md)
            if let Some(label) = filter {
                span { class: "filter-label", "{label}" }
            }
            if loops > 0 {
                span {
                    class: "ember",
                    onclick: move |_| on_ember.call(()),
                    "{loops}"
                }
            }
        }
    }
}

/// What one look at the index yields: the rail's time notes, the open loops
/// themselves rather than a count of them, the table's cards-to-be, and the
/// link edges the canvas draws between them.
type Survey = (
    Vec<(String, NoteType)>,
    Vec<String>,
    Vec<TableNote>,
    Vec<(String, String)>,
);

/// What the shell mounts with: the vault root and that survey.
type Loaded = (
    PathBuf,
    Vec<(String, NoteType)>,
    Vec<String>,
    Vec<TableNote>,
    Vec<(String, String)>,
);

fn load(root: Option<PathBuf>) -> Result<Loaded, String> {
    match root {
        Some(root) => match load_notes(&root) {
            Ok((notes, loops, table, edges)) => {
                Ok((root, notes, loops, table, edges))
            }
            Err(err) => Err(format!("the index could not be built: {err:?}")),
        },
        None => Err("no vault: define NOTE_VAULT or HOME".to_string()),
    }
}

fn load_notes(root: &Path) -> Result<Survey, IndexError> {
    let notes = crate::index::scan_vault(root)?;
    let index_path = root.join(".index");
    std::fs::create_dir_all(&index_path)?;
    let mut index = Index::open(&index_path.join("index.db"))?;
    index.rebuild(&notes)?;
    survey(&index)
}

/// What the shell needs from a built index: the rail's time notes and the
/// ember's count. Separate from `load_notes` so its error arms stay
/// reachable — after a successful rebuild they only fire on a sabotaged
/// database.
fn survey(index: &Index) -> Result<Survey, IndexError> {
    Ok((
        index.time_notes()?,
        open_loops(index)?,
        index.table_notes()?,
        index.link_edges()?,
    ))
}

/// One watcher batch applied: the index catches up with the files, then the
/// screen catches up with the index. Opening the index per batch matches
/// every other read in this module — the batches arrive debounced, seldom,
/// and one at a time.
fn refresh(
    root: &Path,
    batch: &[watch::VaultChange],
) -> Result<Survey, String> {
    absorb(root, batch).map_err(|err| format!("watching the vault: {err:?}"))
}

/// Open, apply, re-read: every step reports the same way, so the caller has
/// one message to show rather than three.
fn absorb(
    root: &Path,
    batch: &[watch::VaultChange],
) -> Result<Survey, IndexError> {
    let mut index = Index::open(&root.join(".index/index.db"))?;
    watch::apply(&mut index, root, batch)?;
    survey(&index)
}

/// The open loops themselves, not a count of them: the chrome's ember shows
/// this list's length and clicking it shows the list, so the two cannot
/// drift apart (adr/2026-08-loops-list-overlay.md).
fn open_loops(index: &Index) -> Result<Vec<String>, IndexError> {
    Ok(loops::lines(
        &index.typeless_notes()?,
        &index.dangling_links()?,
        &index.unsummarized_captures()?,
    ))
}

/// The selected note's editor: opened when the index says the note exists,
/// closed otherwise — selection ≠ existence, so an empty selection must not
/// touch the filesystem.
fn open_selected(root: &Path, exists: bool, id: &str) -> Editor {
    if exists {
        Editor::open(time_note_path(root, id))
    } else {
        Editor::closed()
    }
}

/// A time note's id is its stem, so the path needs no index round-trip.
fn time_note_path(root: &Path, id: &str) -> PathBuf {
    root.join(NoteCategory::Time.as_dir())
        .join(format!("{id}.typ"))
}

/// The sheet's editor: the card knows its id, not its file, so the path is
/// looked up per event — the `completions` pattern. A lookup that fails
/// still opens the sheet: a closed editor carrying the error puts the
/// message where the user is looking, and Escape closes it.
fn open_sheet_note(root: &Path, id: &str) -> Editor {
    let looked_up = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("sheet: {err:?}"))
        .and_then(|index| {
            index
                .path_for_id(&crate::domain::NoteId(id.to_string()))
                .map_err(|err| format!("sheet: {err:?}"))
        });
    match looked_up {
        Ok(Some(path)) => Editor::open(root.join(path)),
        Ok(None) => closed_with(format!("sheet: no note has the id {id}")),
        Err(message) => closed_with(message),
    }
}

/// A closed editor already carrying its notice — the sheet's error state.
fn closed_with(message: String) -> Editor {
    let mut editor = Editor::closed();
    editor.set_notice(message);
    editor
}

/// The sheet's footer: how many notes link here — a count ("← 2"), the
/// card vocabulary, where the logs footer lists ids
/// (adr/2026-08-links-footer-both-directions.md rejected the count there,
/// which is exactly why it holds here). Scales don't matter to a count, so
/// the classifier gets no time notes.
fn sheet_backlinks(root: &Path, own: &str) -> Result<usize, String> {
    let index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("backlinks: {err:?}"))?;
    let sources = index
        .backlinks(&crate::domain::NoteId(own.to_string()))
        .map_err(|err| format!("backlinks: {err:?}"))?;
    Ok(links::backlinks(&sources, own, &[]).len())
}

/// One centre-pane slot: the active block as raw source for the textarea,
/// every other as its cached fragment, tagged with its start byte so a
/// click activates by coordinate rather than by shiftable index.
enum Pane {
    Source {
        start: usize,
        text: String,
    },
    Fragment {
        start: usize,
        rendered: Result<String, String>,
    },
}

fn block_panes(
    editor: &Editor,
    root: &Path,
    theme: RenderTheme,
    cache: &mut FragmentCache,
) -> Option<Vec<Pane>> {
    let (file, text) = editor.note()?;
    Some(
        editor
            .blocks()
            .iter()
            .enumerate()
            .map(|(index, block)| {
                let start = block.range.start;
                if editor.active() == Some(index) {
                    Pane::Source {
                        start,
                        text: text
                            .get(block.content())
                            .unwrap_or("")
                            .to_string(),
                    }
                } else {
                    Pane::Fragment {
                        start,
                        rendered: cache.render(
                            root,
                            file,
                            &blocks::fragment_source(text, block),
                            theme,
                        ),
                    }
                }
            })
            .collect(),
    )
}

/// The open link picker's fixed half: the index snapshot the query filters,
/// taken once at open because nothing can change it while the popup holds
/// focus (adr/2026-08-ctrl-l-link-picker.md). The splice point is the caret
/// itself — app state no overlay can move, so nothing needs freezing
/// (adr/2026-08-caret-on-editor-note-bytes.md). The moving half — query and
/// highlight — lives in its own signals.
#[derive(Clone, PartialEq)]
struct Picker {
    entries: Vec<links::Completion>,
}

/// The open command palette's fixed half — the `Picker` idiom.
/// `block_active` decides which commands exist at all
/// (adr/2026-08-palette-birth-command-list.md); the caret commands run
/// against the editor's own caret, which the palette cannot move. The
/// moving half — query and highlight — lives in its own signals.
#[derive(Clone, Copy, PartialEq)]
struct Palette {
    block_active: bool,
    /// Which screen the palette opened over: the screen commands hide where
    /// they already stand (adr/2026-08-screen-switch-gesture.md).
    on_table: bool,
    /// Whether a sheet was open: delete exists only over one
    /// (adr/2026-08-delete-note-palette-only-from-sheet.md).
    sheet_open: bool,
    /// Whether the table stood at body zoom: each zoom command hides at its
    /// own level (adr/2026-08-body-zoom-scale-and-metrics.md).
    at_bodies: bool,
}

/// The open create overlay's fixed half — the `Palette` idiom with one more
/// fact: which step it stands in. `picked: None` is step 1 (the type list);
/// `Some` is step 2, where the same input is the title prompt
/// (adr/2026-08-ctrl-n-two-step-create-overlay.md).
#[derive(Clone, PartialEq)]
struct Creator {
    picked: Option<NoteType>,
}

/// The open filter overlay's fixed half: its vocabulary — every tag, then
/// the eight types — frozen at open (adr/2026-08-filter-overlay-ctrl-f.md).
#[derive(Clone, PartialEq)]
struct FilterPicker {
    entries: Vec<table::FilterEntry>,
}

/// The open jump overlay's fixed half — the `Picker` idiom without an
/// anchor (adr/2026-08-jump-ctrl-o-centres-viewport.md).
#[derive(Clone, PartialEq)]
struct Jump {
    entries: Vec<links::Completion>,
}

/// Every tag the filter can offer, read at the moment the overlay opens —
/// the `completions` pattern.
fn tag_names(root: &Path) -> Result<Vec<String>, String> {
    let index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("filter: {err:?}"))?;
    index.tag_names().map_err(|err| format!("filter: {err:?}"))
}

/// Everything the picker can offer, read at the moment it opens.
fn completions(root: &Path) -> Result<Vec<links::Completion>, String> {
    let index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("links: {err:?}"))?;
    Ok(index
        .completions()
        .map_err(|err| format!("links: {err:?}"))?
        .into_iter()
        .map(links::Completion::new)
        .collect())
}

/// The both-directions footer under the open note, or `None` when no note is
/// open — absence, not an empty row. Outgoing links are read from the live
/// buffer so a link marks itself dangling as it is typed; backlinks come
/// from the index, which no in-session edit can change
/// (adr/2026-08-links-footer-both-directions.md).
type Footer = (Vec<links::FooterLink>, Vec<links::FooterLink>);

fn link_footer(
    root: &Path,
    editor: &Editor,
    own: &str,
    time_notes: &[(String, NoteType)],
) -> Option<Result<Footer, String>> {
    let (_, text) = editor.note()?;
    Some(both_directions(root, text, own, time_notes))
}

fn both_directions(
    root: &Path,
    text: &str,
    own: &str,
    time_notes: &[(String, NoteType)],
) -> Result<Footer, String> {
    let index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("links: {err:?}"))?;
    let targets: Vec<crate::domain::NoteId> = crate::parse::parse_note(text)
        .links
        .into_iter()
        .map(|link| link.target)
        .collect();
    // resolved up front rather than inside the classifier, so a database
    // that cannot answer is an error rather than a note full of ghosts
    let mut known = Vec::new();
    for target in &targets {
        if index
            .path_for_id(target)
            .map_err(|err| format!("links: {err:?}"))?
            .is_some()
        {
            known.push(target.0.clone());
        }
    }
    let out = links::outgoing(
        &targets,
        own,
        |id| known.iter().any(|found| found == id),
        time_notes,
    );
    let sources = index
        .backlinks(&crate::domain::NoteId(own.to_string()))
        .map_err(|err| format!("links: {err:?}"))?;
    Ok((links::backlinks(&sources, own, time_notes), out))
}

/// The "captured today" block: the capture and generated notes the index
/// dates to `day`. Opened per read, the same pattern as any other
/// per-event index use — the day gathers what happened in it.
fn captured_lines(root: &Path, day: &str) -> Result<Vec<String>, String> {
    let index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("captured today: {err:?}"))?;
    let captured = index
        .captured_on(day)
        .map_err(|err| format!("captured today: {err:?}"))?;
    // a capture that still owes its summary says so rather than naming its
    // category — the same debt the open-loops list counts
    let open: HashSet<String> = index
        .unsummarized_captures()
        .map_err(|err| format!("captured today: {err:?}"))?
        .iter()
        .map(|path| crate::domain::stem_of(path))
        .collect();
    Ok(captured
        .iter()
        .map(|(stem, category)| {
            logs::captured_line(stem, category, open.contains(stem))
        })
        .collect())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::any::Any;
    use std::rc::Rc;
    use std::sync::atomic::Ordering;

    use dioxus::dioxus_core::{ElementId, Event, Mutation, Mutations};
    use dioxus::html::*;
    use dioxus::prelude::VirtualDom;

    use super::*;

    /// Only a typst-rendered note carries the SVG namespace — the chrome's
    /// rsx icons don't — so this is the "a note is rendered" marker.
    const RENDERED_NOTE: &str = r#"xmlns="http://www.w3.org/2000/svg""#;

    /// The tests' clock: a Thursday inside the fixture week, so the initial
    /// selection is `time/2026-07-23.typ` and the grid opens on july 2026.
    const TODAY: &str = "2026-07-23";

    /// Initial click-listener layout, established empirically (see the
    /// mounted-app doc): registration runs the chrome's two icons first,
    /// then jump-panel — the header's ‹ today › buttons, the three seasons,
    /// then each grid row as gutter + day cells — then the note's two
    /// link-footer entries, the centre's two inactive blocks (today's
    /// preamble and heading), the two crumb jumps, and finally the five
    /// rail rows top to bottom.
    const CHROME_TABLE: usize = 0;
    const CHROME_LOGS: usize = 1;
    const CAL_BACK: usize = 2;
    const CAL_TODAY: usize = 3;
    const CAL_FORWARD: usize = 4;
    const SEASON_AUTUMN: usize = 7;
    const GUTTER_W31: usize = 38;
    const FOOTER_BACKLINK: usize = 44;
    const FOOTER_OUTGOING: usize = 45;
    const BLOCK_PREAMBLE: usize = 46;
    const CRUMB_WEEK: usize = 47;
    const RAIL_SUMMER: usize = 49;
    const RAIL_W30: usize = 50;
    const RAIL_DAY_23: usize = 51;
    const RAIL_DAY_22: usize = 52;
    const RAIL_DAY_21: usize = 53;
    /// July 2026 leads with two blanks, so a date's cell index is offset by
    /// one gutter per started week row (and everything sits behind the two
    /// chrome icons).
    const fn day_cell(day: usize) -> usize {
        8 + (day + 1) / 7 + day
    }
    /// Which keydown listener is the logs pane's (the other is the root).
    const LOGS_KEYS: usize = 1;
    /// The ember registers with the chrome ahead of everything in the pane,
    /// so in a vault with open loops it takes click listener 2 and every
    /// pane index above shifts by one — which is why the base `temp_vault`
    /// has none.
    const EMBER: usize = 2;

    // -- the App component, driven headlessly through a VirtualDom ----------

    #[test]
    fn without_a_vault_the_app_shows_the_vault_error() {
        let (dom, clicks, _, _) = rendered_app(None);
        let html = dioxus_ssr::render(&dom);
        assert!(clicks.is_empty(), "{html}");
        assert!(html.contains("vault-error"), "{html}");
        assert!(html.contains("no vault: define NOTE_VAULT or HOME"));
    }

    #[test]
    fn an_unbuildable_index_shows_the_vault_error() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let (dom, _, _, _) = rendered_app(Some(dir.path().join("missing")));
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

    // -- the quit chord: close, nothing to flush -----------------------------

    #[test]
    fn ctrl_q_asks_the_window_to_close() {
        let vault = temp_vault();
        let (mut dom, _, keydown, closed) =
            quit_app(Some(vault.path().to_path_buf()));
        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        assert!(closed.load(Ordering::SeqCst));
    }

    #[test]
    fn leaving_the_note_hands_focus_back_to_the_pane() {
        // the window's chords only reach the app root by bubbling, so
        // something inside the app must hold focus; with the cursor always
        // in an open note, only an empty selection frees the pane to take
        // it (adr/2026-08-cursor-always-in-the-note.md)
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let clicks = listeners(&mutations, "click");
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );
        let taken = focused.load(Ordering::SeqCst);

        // day 24 has no note: the editor closes and the pane reclaims
        click(&mut dom, clicks[day_cell(24)]);
        block_on(settle(&mut dom));
        assert!(
            focused.load(Ordering::SeqCst) > taken,
            "the pane asked for focus once the note left"
        );
    }

    #[test]
    fn the_open_loops_list_takes_focus_so_escape_can_close_it() {
        let vault = debt_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let clicks = listeners(&mutations, "click");
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );
        let taken = focused.load(Ordering::SeqCst);

        click(&mut dom, clicks[EMBER]);
        block_on(settle(&mut dom));
        assert!(
            focused.load(Ordering::SeqCst) > taken,
            "clicking the ember leaves focus on the pane that owns escape"
        );
    }

    #[test]
    fn ctrl_q_on_the_vault_error_screen_closes_immediately() {
        let (mut dom, _, keydown, closed) = quit_app(None);
        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        assert!(closed.load(Ordering::SeqCst));
    }

    #[test]
    fn ctrl_q_flushes_the_unsaved_buffer_then_closes() {
        let vault = temp_vault();
        let (mut dom, clicks, keydown, closed) =
            quit_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        // typed but inside the quiet window: only the flush can save it
        retype(&mut dom, sink, "= presque perdu\n");
        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );

        assert!(closed.load(Ordering::SeqCst));
        let saved =
            std::fs::read_to_string(vault.path().join("time/2026-07-23.typ"))
                .expect("the note is readable");
        assert!(saved.contains("presque perdu"), "{saved}");
    }

    #[test]
    fn a_failed_flush_cancels_the_quit_and_shows_the_error() {
        let vault = temp_vault();
        let (mut dom, clicks, keydown, closed) =
            quit_app(Some(vault.path().to_path_buf()));
        let (input, _) = activate_heading(&mut dom, &clicks);
        type_into(&mut dom, input, "= pas encore sauvé\n");

        let file = vault.path().join("time/2026-07-23.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        assert!(
            !closed.load(Ordering::SeqCst),
            "the app never closes over an unsaved buffer"
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("2026-07-23.typ"), "{html}");
    }

    // -- the debounced autosave ----------------------------------------------

    #[test]
    fn typing_then_idling_saves_without_leaving_the_block() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        retype(&mut dom, sink, "= autosauvé\n");
        block_on(settle(&mut dom));

        let saved =
            std::fs::read_to_string(vault.path().join("time/2026-07-23.typ"))
                .expect("the note is readable");
        assert!(saved.contains("autosauvé"), "{saved}");
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("block-active"),
            "the block stays active — saving is not deactivation: {html}"
        );
    }

    #[test]
    fn a_failing_autosave_surfaces_its_error_once() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        let file = vault.path().join("time/2026-07-23.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        // the settle loop spans several autosave restarts, so the
        // value-gated write is exercised on both of its sides here
        retype(&mut dom, sink, "= en panne\n");
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("2026-07-23.typ"), "{html}");
    }

    #[test]
    fn ctrl_q_without_a_window_to_close_is_harmless() {
        // the headless default: no Closer in context, the chord is a no-op
        let (mut dom, keydown) = theme_app();
        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="dark""#), "{html}");
    }

    // -- the chrome: two icons and the ember ---------------------------------

    /// The chrome alone, so the table icon — which no screen mounts before
    /// v1 — can be rendered. The ember handler has to be built inside a
    /// running dom, which is what this wrapper is for.
    #[component]
    fn BareChrome(screen: Screen) -> Element {
        rsx! {
            Chrome {
                screen,
                loops: 0,
                on_ember: move |()| {},
                on_table: move |()| {},
                on_logs: move |()| {},
            }
        }
    }

    #[test]
    fn the_lit_icon_follows_the_current_screen() {
        for (screen, lit, dim) in [
            (Screen::Table, "icon-table lit", "icon-logs lit"),
            (Screen::Logs, "icon-logs lit", "icon-table lit"),
        ] {
            let mut dom = VirtualDom::new_with_props(
                BareChrome,
                BareChromeProps { screen },
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
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("ember"), "absence, not a zero: {html}");
    }

    #[test]
    fn the_ember_shows_the_open_loop_count() {
        let vault = debt_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        // one loop of each kind, and the count is the list's own length
        assert!(html.contains(r#"class="ember">3</span>"#), "{html}");
    }

    // -- the two screens: icons, chords, palette entries ---------------------

    #[test]
    fn the_icons_swap_the_screen_and_back() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(html.contains("icon-logs lit"), "{html}");

        let mutations = click_for_mutations(&mut dom, clicks[CHROME_TABLE]);
        // the pane asks for focus on mount, like the textarea it replaces
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="table""#), "{html}");
        assert!(html.contains("icon-table lit"), "{html}");
        assert!(!html.contains(r#"class="logs""#), "{html}");

        click(&mut dom, clicks[CHROME_LOGS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(html.contains("icon-logs lit"), "{html}");
        assert!(!html.contains(r#"class="table""#), "{html}");
    }

    #[test]
    fn ctrl_1_and_ctrl_2_switch_screens() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // a bare 1 is just typing; only the chord travels — and Ctrl+2
        // where the logs already stand goes nowhere
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("1".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        assert!(dioxus_ssr::render(&dom).contains(r#"class="logs""#));
        let mutations = press_for_mutations(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("1".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="table""#), "{html}");

        // the way back rides the table pane's own keydown; the chord for
        // the screen already stood on stays where it is
        let table_keys = listeners(&mutations, "keydown")[0];
        press(
            &mut dom,
            table_keys,
            Key::Character("1".into()),
            Modifiers::CONTROL,
        );
        press(
            &mut dom,
            table_keys,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        assert!(dioxus_ssr::render(&dom).contains(r#"class="table""#));
        press(
            &mut dom,
            table_keys,
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
    }

    #[test]
    fn leaving_the_logs_keeps_the_cursor() {
        // the cursor belongs to the note: a screen round trip finds it
        // again (adr/2026-08-cursor-always-in-the-note.md)
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        assert!(dioxus_ssr::render(&dom).contains("block-active"));

        click(&mut dom, clicks[CHROME_TABLE]);
        assert!(!dioxus_ssr::render(&dom).contains("block-active"));
        click(&mut dom, clicks[CHROME_LOGS]);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("block-active"),
            "the cursor survived the round trip: {html}"
        );
    }

    #[test]
    fn the_palette_switches_screens_by_name() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "table");
        let mutations = press_for_mutations(
            &mut dom,
            palette_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="table""#), "{html}");

        // from the table the palette offers the way back and nothing the
        // table cannot answer for — the logs' block is active but hidden
        // behind the screen, so the caret commands hide with it
        let table_keys = listeners(&mutations, "keydown")[0];
        let (input, palette_keys) = open_palette(&mut dom, table_keys);
        // a second Ctrl+P while it is open changes nothing
        press(
            &mut dom,
            table_keys,
            Key::Character("p".into()),
            Modifiers::CONTROL,
        );
        let labels = palette_labels(&dom);
        assert!(labels.contains(&"go to logs".to_string()), "{labels:?}");
        assert!(!labels.contains(&"go to table".to_string()), "{labels:?}");
        assert!(!labels.contains(&"insert link".to_string()), "{labels:?}");

        type_into(&mut dom, input, "logs");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
    }

    // -- the table at rest: cards, kinds, liveness (wireframe state 6a) ------

    #[test]
    fn every_kind_wears_its_treatment_on_the_table() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        let html = dioxus_ssr::render(&dom);

        // permanent: filled card, its type's bar, the type as label
        assert!(html.contains("card card-permanent bar-concept"), "{html}");
        assert!(html.contains(">concept</div>"), "{html}");
        assert!(html.contains(">alpha</div>"), "the title shows: {html}");
        // typeless permanent: the grey bar of visible debt
        assert!(html.contains("card card-permanent bar-untyped"), "{html}");
        assert!(html.contains(">untyped</div>"), "{html}");
        // capture: dimmer treatment and the friction age in the label
        assert!(html.contains("card card-capture bar-untyped"), "{html}");
        assert!(html.contains(">capture · 0 d</div>"), "{html}");
        // generated: dashed all round, its own label, no hue
        assert!(html.contains("card card-generated bar-generated"), "{html}");
        assert!(html.contains(">generated</div>"), "{html}");
        // and the one category that never appears
        assert!(!html.contains("2026-07-23"), "no time notes: {html}");
    }

    #[test]
    fn unplaced_cards_stack_on_the_origin_grid_the_same_way_twice() {
        let vault = temp_vault();
        let render_table = || {
            let (mut dom, clicks, _, _) =
                rendered_app(Some(vault.path().to_path_buf()));
            click(&mut dom, clicks[CHROME_TABLE]);
            dioxus_ssr::render(&dom)
        };
        let first = render_table();
        // nothing is placed yet: ids fill the grid in order, ×4 spacing
        assert!(first.contains("left: 32px; top: 32px"), "{first}");
        assert!(first.contains("left: 224px; top: 32px"), "{first}");
        assert!(first.contains("left: 416px; top: 32px"), "{first}");
        assert_eq!(first, render_table(), "the fallback cannot shuffle");
    }

    #[test]
    fn notes_written_outside_keep_the_table_live() {
        let vault = temp_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        assert!(!dioxus_ssr::render(&dom).contains("beta"));

        let path = vault.path().join("permanent/beta.typ");
        std::fs::write(&path, note("beta")).expect("the note is written");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Permanent,
                path: PathBuf::from("permanent/beta.typ"),
            }],
        );
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(">beta</div>"),
            "the new card appears without a relaunch: {html}"
        );

        std::fs::remove_file(&path).expect("the note is deleted");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Removed(PathBuf::from(
                "permanent/beta.typ",
            ))],
        );
        assert!(
            !dioxus_ssr::render(&dom).contains("beta"),
            "and leaves when the file does"
        );
    }

    // -- drag, pan, and the debounced write to the positions file ------------

    #[test]
    fn dragging_a_card_moves_it_and_the_debounce_writes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        // alpha is the first card in id order, on the grid at (32, 32)
        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (140.0, 88.0));
        mouse(&mut dom, "mousemove", pane, (150.0, 90.0));
        mouse(&mut dom, "mouseup", pane, (150.0, 90.0));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("left: 82px; top: 22px"),
            "the card followed both moves: {html}"
        );
        // the other unplaced cards kept the slots they were first given —
        // placing one never shuffles the rest
        assert!(html.contains("left: 224px; top: 32px"), "{html}");
        assert!(html.contains("left: 416px; top: 32px"), "{html}");

        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert_eq!(saved.trim(), "alpha 82 22");
    }

    #[test]
    fn a_click_that_never_moves_writes_no_position() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mouseup", pane, (100.0, 100.0));
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the idle tick still writes the empty store");
        assert!(!saved.contains("alpha"), "no movement, no entry: {saved}");
    }

    #[test]
    fn panning_moves_the_canvas_and_writes_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, _) = table_targets(&mut dom, &clicks);

        // a stray move with nothing held moves nothing
        mouse(&mut dom, "mousemove", pane, (10.0, 10.0));
        assert!(
            dioxus_ssr::render(&dom)
                .contains("transform: scale(1) translate(0px, 0px)")
        );

        mouse(&mut dom, "mousedown", pane, (200.0, 200.0));
        mouse(&mut dom, "mousemove", pane, (180.0, 230.0));
        mouse(&mut dom, "mouseup", pane, (180.0, 230.0));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("transform: scale(1) translate(-20px, 30px)"),
            "the void panned: {html}"
        );
        assert!(
            html.contains("left: 32px; top: 32px"),
            "the cards kept their canvas coordinates: {html}"
        );

        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the idle tick still writes the empty store");
        assert_eq!(saved.trim(), "", "panning is not placement: {saved}");
    }

    #[test]
    fn a_dragged_position_survives_the_quit_and_the_relaunch() {
        let vault = temp_vault();
        let (mut dom, clicks, keydown, closed) =
            quit_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        mouse(&mut dom, "mousedown", cards[0], (0.0, 0.0));
        mouse(&mut dom, "mousemove", pane, (300.0, 250.0));
        mouse(&mut dom, "mouseup", pane, (300.0, 250.0));

        // quit inside the quiet window: only the flush can save it
        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        assert!(closed.load(Ordering::SeqCst), "the flush let the quit by");

        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("left: 332px; top: 282px"),
            "drag, quit, relaunch — it stayed put: {html}"
        );
    }

    #[test]
    fn an_unwritable_positions_file_surfaces_and_holds_the_quit() {
        let vault = temp_vault();
        // the store's path is a directory: the load degrades to "nothing
        // placed", and every save fails
        std::fs::create_dir_all(vault.path().join(".index/positions"))
            .expect("the sabotage directory is created");
        let (mut dom, clicks, keydown, closed) =
            quit_app(Some(vault.path().to_path_buf()));

        // the first idle tick already fails; the notice shows in the logs
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("positions:"), "{html}");

        // a drag re-fails without re-setting the identical notice
        let (pane, cards) = table_targets(&mut dom, &clicks);
        mouse(&mut dom, "mousedown", cards[0], (0.0, 0.0));
        mouse(&mut dom, "mousemove", pane, (40.0, 40.0));
        mouse(&mut dom, "mouseup", pane, (40.0, 40.0));
        block_on(settle(&mut dom));

        // and the flush failure cancels the quit instead of losing the drag
        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        assert!(
            !closed.load(Ordering::SeqCst),
            "an unsaved store holds the app open"
        );
    }

    // -- the writing sheet: click to open, escape to close (wireframe 6b) ----

    #[test]
    fn a_click_on_a_card_opens_its_sheet_dimmed_and_tethered() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        open_sheet_on(&mut dom, pane, cards[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="dim""#), "{html}");
        assert!(html.contains("raised"), "{html}");
        // alpha at (32, 32): its right edge to the sheet's left edge
        assert!(
            html.contains(
                r#"class="tether" style="left: 208px; top: 60px; width: 232px""#
            ),
            "{html}"
        );
        assert!(
            html.contains(r#"style="left: 440px; width: 620px""#),
            "{html}"
        );
        // the origin card left the canvas for its raised copy — one alpha
        assert_eq!(html.matches(">alpha</div>").count(), 1, "{html}");
        // the note renders in the sheet, and with no backlinks the footer
        // is absent — the ember's idiom
        assert!(html.contains(RENDERED_NOTE), "{html}");
        assert!(!html.contains("sheet-footer"), "{html}");
    }

    #[test]
    fn every_kind_opens_a_sheet() {
        let vault = temp_vault();
        // alpha (permanent), capture-idea (capture), digest (generated) —
        // a fresh mount each: closing remounts cards under new element
        // ids, so a second open cannot reuse the first harvest
        for rank in 0..3 {
            let (mut dom, clicks, _, _) =
                rendered_app(Some(vault.path().to_path_buf()));
            let (pane, cards) = table_targets(&mut dom, &clicks);
            open_sheet_on(&mut dom, pane, cards[rank]);
            let html = dioxus_ssr::render(&dom);
            assert!(
                html.contains(r#"class="sheet""#),
                "card {rank} opened no sheet: {html}"
            );
            assert!(html.contains(RENDERED_NOTE), "{html}");
        }
    }

    #[test]
    fn a_drag_beyond_the_slop_opens_no_sheet() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (150.0, 90.0));
        mouse(&mut dom, "mouseup", pane, (150.0, 90.0));
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "{html}");
    }

    #[test]
    fn a_jittered_click_inside_the_slop_opens_and_still_writes() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        // three pixels of wobble: a click by the slop, and an honest move
        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (103.0, 98.0));
        mouse(&mut dom, "mouseup", pane, (103.0, 98.0));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="sheet""#), "{html}");

        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert_eq!(saved.trim(), "alpha 35 30", "the wobble wrote: {saved}");
    }

    #[test]
    fn escape_closes_the_sheet_and_restores_the_day() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(!html.contains(r#"class="dim""#), "{html}");
        assert!(!html.contains("raised"), "{html}");
        // the card went back to its slot in the canvas
        assert!(html.contains("left: 32px; top: 32px"), "{html}");
        // a second escape with nothing open lands on no arm
        press(&mut dom, keys, Key::Escape, Modifiers::empty());

        // the one editor holds the day again: the logs render it
        press(
            &mut dom,
            keys,
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn escape_in_a_sheet_block_closes_the_sheet_directly() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, block_keys) = sheet_block_targets(&opened);
        assert!(dioxus_ssr::render(&dom).contains("block-active"));

        // the block no longer swallows escape: one press bubbles to the
        // pane and the sheet closes
        // (adr/2026-08-cursor-always-in-the-note.md)
        press(&mut dom, block_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        // and the logs' note wakes with its own cursor
        press(
            &mut dom,
            keys,
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        assert!(dioxus_ssr::render(&dom).contains("block-active"));
    }

    #[test]
    fn the_tether_tracks_the_card_through_drag_and_pan() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        // document order above the dim: the raised card, then the aside
        let raised = listeners(&opened, "mousedown")[0];

        // dragging the raised card drags the tether's card end
        mouse(&mut dom, "mousedown", raised, (0.0, 0.0));
        mouse(&mut dom, "mousemove", pane, (10.0, 20.0));
        mouse(&mut dom, "mouseup", pane, (10.0, 20.0));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("left: 218px; top: 80px; width: 222px"),
            "the tether followed the drag: {html}"
        );

        // panning under the sheet moves card and tether together
        mouse(&mut dom, "mousedown", pane, (200.0, 200.0));
        mouse(&mut dom, "mousemove", pane, (190.0, 180.0));
        mouse(&mut dom, "mouseup", pane, (190.0, 180.0));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("left: 208px; top: 60px; width: 232px"),
            "the tether followed the pan: {html}"
        );
        assert!(
            html.contains(r#"style="left: 440px; width: 620px""#),
            "the sheet stood still: {html}"
        );
    }

    #[test]
    fn clicking_the_raised_card_reopens_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let raised = listeners(&opened, "mousedown")[0];

        let before = dioxus_ssr::render(&dom);
        mouse(&mut dom, "mousedown", raised, (50.0, 50.0));
        mouse(&mut dom, "mouseup", pane, (50.0, 50.0));
        assert_eq!(
            dioxus_ssr::render(&dom),
            before,
            "the open sheet is already this card's"
        );
    }

    #[test]
    fn go_logs_with_a_sheet_open_closes_it_first() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        press(
            &mut dom,
            keys,
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        assert!(dioxus_ssr::render(&dom).contains(r#"class="logs""#));

        // back on the table — by the chrome icon, whose element outlives
        // the pane the chord listener left with — no sheet waits behind
        click(&mut dom, clicks[CHROME_TABLE]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="table""#), "{html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");
    }

    #[test]
    fn the_sheet_survives_its_card_vanishing() {
        let vault = temp_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        std::fs::remove_file(vault.path().join("permanent/alpha.typ"))
            .expect("the note is deleted");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Removed(PathBuf::from(
                "permanent/alpha.typ",
            ))],
        );
        let html = dioxus_ssr::render(&dom);
        // no card, no tether — but the sheet and its buffer hold: the
        // watcher never reloads the open note
        assert!(!html.contains("raised"), "{html}");
        assert!(!html.contains("tether"), "{html}");
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn a_cardless_id_opens_the_sheet_with_a_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        // the card is drawn from a signal the sabotage cannot reach; the
        // click's lookup is what meets the missing row
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute("DELETE FROM notes WHERE id = 'alpha'", [])
            .expect("the sabotage succeeds");

        open_sheet_on(&mut dom, pane, cards[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains("sheet: no note has the id alpha"), "{html}");
    }

    #[test]
    fn an_unopenable_index_surfaces_in_the_sheet() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        replace_database_with_a_directory(vault.path());

        open_sheet_on(&mut dom, pane, cards[0]);
        let html = dioxus_ssr::render(&dom);
        // both per-event reads say so: the note lookup and the footer count
        assert!(html.contains("sheet: "), "{html}");
        assert!(html.contains("backlinks: "), "{html}");
    }

    #[test]
    fn a_sheet_over_a_missing_notes_table_reports_the_lookup_error() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        // the index opens but cannot answer: the lookup's own error arm
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE notes")
            .expect("the sabotage succeeds");

        open_sheet_on(&mut dom, pane, cards[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("sheet: "), "{html}");
    }

    #[test]
    fn a_sheet_over_a_missing_links_table_reports_the_footer_error() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        // the notes table still answers, so the sheet opens — only the
        // backlink count has nothing to read
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE links")
            .expect("the sabotage succeeds");

        open_sheet_on(&mut dom, pane, cards[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "the note renders: {html}");
        assert!(html.contains("backlinks: "), "{html}");
    }

    #[test]
    fn an_unsavable_sheet_holds_open_rather_than_losing_text() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        let file = vault.path().join("permanent/alpha.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="sheet""#),
            "an unsaved buffer holds the sheet open: {html}"
        );
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("alpha.typ"), "{html}");
    }

    #[test]
    fn a_sheet_refuses_to_open_over_an_unsavable_buffer() {
        let vault = temp_vault();
        // alpha links today, so today's footer carries a clickable backlink
        std::fs::write(
            vault.path().join("permanent/alpha.typ"),
            linking(note("alpha"), "2026-07-23"),
        )
        .expect("alpha is rewritten");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let file = vault.path().join("time/2026-07-23.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        // the flush guard refuses: the day's buffer cannot reach disk, so
        // the logs stay up with the error rather than dropping the buffer
        click(&mut dom, clicks[FOOTER_BACKLINK]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains("render-error"), "{html}");
    }

    #[test]
    fn editing_in_the_sheet_saves_through_the_autosave_watcher_and_index() {
        let vault = temp_vault();
        // beta links alpha, so alpha's sheet opens with one backlink
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            linking(note("beta"), "alpha"),
        )
        .expect("beta is written");
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        assert!(
            dioxus_ssr::render(&dom).contains(r#"sheet-footer">← 1"#),
            "{}",
            dioxus_ssr::render(&dom)
        );

        let (_, sink) = sheet_block_targets(&opened);
        retype(&mut dom, sink, "= alpha renommé\n");
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join("permanent/alpha.typ"))
                .expect("the note is readable");
        assert!(saved.contains("alpha renommé"), "{saved}");

        // the watcher's re-index repaints the card while the buffer holds
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Permanent,
                path: PathBuf::from("permanent/alpha.typ"),
            }],
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">alpha renommé</div>"), "{html}");
        assert!(html.contains("block-active"), "still editing: {html}");
    }

    #[test]
    fn ctrl_q_with_a_sheet_open_flushes_the_buffer() {
        let vault = temp_vault();
        let (mut dom, clicks, keydown, closed) =
            quit_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, sink) = sheet_block_targets(&opened);
        // typed but inside the quiet window: only the flush can save it
        retype(&mut dom, sink, "= presque perdu\n");

        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        assert!(closed.load(Ordering::SeqCst));
        let saved =
            std::fs::read_to_string(vault.path().join("permanent/alpha.typ"))
                .expect("the note is readable");
        assert!(saved.contains("presque perdu"), "{saved}");
    }

    #[test]
    fn ctrl_l_in_the_sheet_opens_the_picker_and_accepts() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (block, keys) = sheet_block_targets(&opened);

        // put the caret at the end of "= alpha"
        place_caret(&mut dom, block, &hit, "= alpha".len());
        let (input, picker_keys) = open_picker(&mut dom, keys);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("link-picker"), "{html}");
        assert!(html.contains(r#"class="sheet""#), "{html}");

        type_into(&mut dom, input, "digest");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "accepting closes it: {html}");
        assert!(
            source_of(&dom).contains(r#"= alpha#l("digest")"#),
            "spliced at the caret: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn a_press_inside_the_sheet_never_pans_the_table() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        // document order above the dim: the raised card, then the aside
        let aside = listeners(&opened, "mousedown")[1];

        // the press stops at the sheet, so the move that follows holds
        // nothing — a text selection inside must not drag the table
        mouse(&mut dom, "mousedown", aside, (500.0, 300.0));
        mouse(&mut dom, "mousemove", pane, (520.0, 320.0));
        mouse(&mut dom, "mouseup", pane, (520.0, 320.0));
        assert!(
            dioxus_ssr::render(&dom)
                .contains("transform: scale(1) translate(0px, 0px)"),
            "{}",
            dioxus_ssr::render(&dom)
        );
    }

    #[test]
    fn ctrl_enter_on_a_permanent_link_opens_the_sheet() {
        let vault = temp_vault();
        // today's heading links alpha instead of yesterday
        std::fs::write(
            vault.path().join("time/2026-07-23.typ"),
            linking(time_note("2026-07-23", "daily"), "alpha"),
        )
        .expect("the day is rewritten");
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);

        place_caret(&mut dom, block, &hit, LINK_IN_HEADING + 3);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="table""#),
            "the chord crossed screens: {html}"
        );
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains("raised"), "{html}");
    }

    /// Beta's heading is `= beta\n#l("alpha")#l("2026-07-22")` — one link
    /// of each reach, for the follows that start inside a sheet.
    fn beta_with_both_links(vault: &Path) {
        std::fs::write(
            vault.join("permanent/beta.typ"),
            format!("{}#l(\"alpha\")#l(\"2026-07-22\")\n", note("beta")),
        )
        .expect("beta is written");
    }

    #[test]
    fn ctrl_enter_in_the_sheet_opens_the_linked_sheet() {
        let vault = temp_vault();
        beta_with_both_links(vault.path());
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        // beta sits second in id order
        let opened = open_sheet_on(&mut dom, pane, cards[1]);
        let (block, keys) = sheet_block_targets(&opened);

        // inside `#l("alpha")`, just past "= beta\n"
        place_caret(&mut dom, block, &hit, 10);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        // the raised card is now alpha's, on alpha's slot; beta went back
        assert!(
            html.contains(r#"raised " style="left: 32px; top: 32px""#),
            "the sheets swapped: {html}"
        );
        assert!(html.contains(">beta</div>"), "{html}");
    }

    #[test]
    fn a_time_link_in_the_sheet_lands_on_the_logs() {
        let vault = temp_vault();
        beta_with_both_links(vault.path());
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[1]);
        let (block, keys) = sheet_block_targets(&opened);

        // inside `#l("2026-07-22")`
        place_caret(&mut dom, block, &hit, 22);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the linked day is selected: {html}"
        );
    }

    #[test]
    fn ctrl_enter_on_a_dangling_link_goes_nowhere() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/gamma.typ"),
            format!("{}#l(\"fantome\")\n", note("gamma")),
        )
        .expect("gamma is written");
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        // gamma sits last in id order
        let opened = open_sheet_on(&mut dom, pane, cards[3]);
        let (block, keys) = sheet_block_targets(&opened);

        // inside `#l("fantome")`, just past "= gamma\n"
        place_caret(&mut dom, block, &hit, 11);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"raised " style="left: 608px; top: 32px""#),
            "gamma's sheet stayed put: {html}"
        );
    }

    #[test]
    fn the_palette_over_a_sheet_offers_the_editor_commands_and_leaves() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        sheet_block_targets(&opened);

        // no new command was registered for phase 3 — the editor commands
        // simply become available where the editor now is
        let (input, palette_keys) = open_palette(&mut dom, keys);
        let labels = palette_labels(&dom);
        for expected in ["insert link", "follow link", "go to logs"] {
            assert!(
                labels.iter().any(|label| label == expected),
                "{labels:?}"
            );
        }
        assert!(
            !labels.iter().any(|label| label == "go to table"),
            "going where you stand is not a command: {labels:?}"
        );

        type_into(&mut dom, input, "go to logs");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(r#"class="logs""#));

        // and the screen switch closed the sheet on its way out
        click(&mut dom, clicks[CHROME_TABLE]);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "{html}");
    }

    #[test]
    fn the_ember_opens_the_flat_list_and_closes_it_again() {
        let vault = debt_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        assert!(
            !dioxus_ssr::render(&dom).contains("loops-list"),
            "the list waits to be asked for"
        );

        click(&mut dom, clicks[EMBER]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("open loops"), "{html}");
        for line in [
            "mystere · typeless",
            "linky → ghost · dangling",
            "capture-zettel · still open",
        ] {
            assert!(html.contains(line), "missing {line}: {html}");
        }

        // a second click puts it away, and so does escape
        click(&mut dom, clicks[EMBER]);
        assert!(!dioxus_ssr::render(&dom).contains("loops-list"));
        click(&mut dom, clicks[EMBER]);
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("loops-list"), "{html}");
        assert!(
            html.contains(r#"class="ember">3</span>"#),
            "the count stays"
        );
    }

    // -- the rail: every time note, newest first, nothing else ---------------

    #[test]
    fn the_rail_lists_time_notes_newest_first_and_nothing_else() {
        let vault = temp_vault();
        let (dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);

        let order = [
            "2026-summer",
            "2026-w30",
            "2026-07-23",
            "2026-07-22",
            "2026-07-21",
        ];
        let positions: Vec<usize> = order
            .iter()
            .map(|id| html.find(id).expect("every time note is listed"))
            .collect();
        assert!(positions.is_sorted(), "newest first: {html}");
        // the wider scales carry their kind tag, days carry none
        assert!(html.contains(">season</span>"), "{html}");
        assert!(html.contains(">week</span>"), "{html}");
        // no list left in the app: the permanent note appears nowhere
        assert!(!html.contains("alpha"), "{html}");
        assert_eq!(
            clicks.len(),
            54,
            "2 chrome icons + 3 header + 3 seasons + 5 gutters + 31 days \
             + 2 footer links + 1 rendered block + 2 crumbs + 5 rail — the \
             active widget listens for presses, not clicks: {html}"
        );
    }

    // -- the centre pane: today at launch, chain, captured today -------------

    #[test]
    fn today_opens_selected_and_rendered_with_its_chain() {
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);

        assert!(html.contains(RENDERED_NOTE), "today renders: {html}");
        for crumb in [">2026-07-23<", ">daily<", ">w30<", ">summer 2026<"] {
            assert!(html.contains(crumb), "missing {crumb}: {html}");
        }
        assert!(html.contains("captured today"), "{html}");
        assert!(html.contains("capture-idea · capture"), "{html}");
        assert!(html.contains("digest · generated"), "{html}");
    }

    #[test]
    fn other_days_carry_no_captured_block() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_22]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "{html}");
        assert!(!html.contains("captured today"), "{html}");
        assert!(html.contains(">2026-07-22<"), "the chain follows: {html}");
    }

    #[test]
    fn a_block_that_cannot_compile_fails_alone() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        // the broken `#let x = (` block wakes as the source — its
        // diagnostic hides while it is the one being edited
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("render-error"), "{html}");
        assert!(html.contains(RENDERED_NOTE), "the preamble renders: {html}");

        // activating the preamble renders the broken block: it fails
        // alone, inline, while the preamble's source stays honest text
        // (day 21's blocks keep day 23's keys, so the mount id holds)
        click(&mut dom, clicks[BLOCK_PREAMBLE]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("#import"), "the preamble source: {html}");
    }

    #[test]
    fn a_vanished_note_shows_the_read_error() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join("time/2026-07-22.typ"))
            .expect("the note exists before the click");
        click(&mut dom, clicks[RAIL_DAY_22]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("2026-07-22.typ"), "{html}");
    }

    #[test]
    fn an_unreadable_index_shows_the_captured_error() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_dir_all(vault.path().join(".index"))
            .expect("the index directory exists");
        // re-selecting today re-runs the captured query against the void
        click(&mut dom, clicks[RAIL_DAY_23]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "the note itself is fine");
        assert!(html.contains("captured today:"), "{html}");
    }

    // -- selection ≠ existence: the empty state and enter --------------------

    #[test]
    fn selecting_an_empty_day_offers_the_template_without_writing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(24)]);
        let html = dioxus_ssr::render(&dom);

        assert!(html.contains("no note for july 24"), "{html}");
        assert!(html.contains("<kbd>enter</kbd>"), "{html}");
        assert!(
            !vault.path().join("time/2026-07-24.typ").exists(),
            "navigating never writes"
        );
        // the rail splices the missing day in, dim, in its slot
        assert!(html.contains("missing"), "{html}");
        let spliced = html.find("2026-07-24").expect("the day is spliced in");
        let neighbour = html.find("2026-07-23").expect("the day before");
        assert!(spliced < neighbour, "newest first keeps the slot: {html}");
    }

    #[test]
    fn enter_creates_the_missing_day_and_renders_it() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(24)]);
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "{html}");
        assert!(!html.contains("missing"), "the rail row is real now");
        let written =
            std::fs::read_to_string(vault.path().join("time/2026-07-24.typ"))
                .expect("enter wrote the note");
        assert!(written.contains("2026-07-24"), "{written}");
        assert!(!written.contains("{{"), "placeholders filled: {written}");
    }

    #[test]
    fn enter_on_an_existing_note_writes_nothing() {
        let vault = temp_vault();
        let file = vault.path().join("time/2026-07-23.typ");
        let before =
            std::fs::read_to_string(&file).expect("the note is readable");
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());
        assert_eq!(
            std::fs::read_to_string(&file).expect("still readable"),
            before
        );
    }

    #[test]
    fn other_keys_on_the_logs_pane_create_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(24)]);
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert!(!vault.path().join("time/2026-07-24.typ").exists());
    }

    #[test]
    fn a_missing_template_reports_the_create_error_and_navigating_clears_it() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join("templates/daily.typ"))
            .expect("remove the template");
        click(&mut dom, clicks[day_cell(24)]);
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("UnknownTemplate"), "{html}");
        assert!(!vault.path().join("time/2026-07-24.typ").exists());

        click(&mut dom, clicks[RAIL_DAY_23]);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("UnknownTemplate"), "navigation clears it");
    }

    #[test]
    fn missing_weeks_and_seasons_create_from_their_period_start() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        click(&mut dom, clicks[GUTTER_W31]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no note for w31"), "{html}");
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());
        let written =
            std::fs::read_to_string(vault.path().join("time/2026-w31.typ"))
                .expect("enter wrote the weekly note");
        assert!(written.contains("2026-07-27"), "the Monday: {written}");

        click(&mut dom, clicks[SEASON_AUTUMN]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no note for autumn 2026"), "{html}");
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());
        let written =
            std::fs::read_to_string(vault.path().join("time/2026-autumn.typ"))
                .expect("enter wrote the seasonal note");
        assert!(written.contains("2026-09-01"), "the first day: {written}");
    }

    // -- the hybrid editor: click to source, type, escape to rendered --------

    #[test]
    fn a_note_opens_with_the_caret_on_its_last_line() {
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        // the heading block "= 2026-07-23\n#l(\"2026-07-22\")\n" ends in a
        // newline, so the caret's line is the empty last one: a source
        // line holding nothing but the drawn caret
        // (adr/2026-08-cursor-always-in-the-note.md)
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"<div class="source-line"><span class="caret">"#),
            "{html}"
        );
        // the renderer announces the caret's mount; the scroll-into-view
        // asks and the headless refusal is absorbed
        mount(&mut dom, listeners(&mutations, "mounted")[2]);
        block_on(settle(&mut dom));
    }

    #[test]
    fn clicking_a_block_opens_its_source_in_place() {
        // the note opens with its heading already the source; clicking the
        // still-rendered preamble moves the source there and renders the
        // heading in its place
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "born editing: {html}");
        assert!(html.contains("= 2026-07-23"), "the raw source: {html}");

        let mutations = click_for_mutations(&mut dom, clicks[BLOCK_PREAMBLE]);
        // the renderer announces the mount and the textarea asks for focus;
        // the fake backing refuses, which is all the handler has to absorb
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "the preamble source: {html}");
        assert!(
            html.contains(RENDERED_NOTE),
            "the heading renders in its place: {html}"
        );
    }

    #[test]
    fn typing_updates_the_buffer_and_escape_writes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);

        retype(&mut dom, keys, "= renamed\n\nencore\n");
        let file = vault.path().join("time/2026-07-23.typ");
        let untouched =
            std::fs::read_to_string(&file).expect("the note is readable");
        assert!(!untouched.contains("renamed"), "typing alone never writes");

        // escape no longer closes the block — the cursor stays put
        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "the cursor held: {html}");

        // the autosave is what writes, once the typing goes quiet
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(&file).expect("the note is readable");
        assert!(saved.contains("= renamed"), "{saved}");
        assert!(saved.contains("encore"), "{saved}");
    }

    #[test]
    fn an_unwritable_note_surfaces_the_flush_error_on_activation() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let file = vault.path().join("time/2026-07-23.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        // activating the preamble flushes the born-active heading first,
        // and the failure is the notice
        click(&mut dom, clicks[BLOCK_PREAMBLE]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("2026-07-23.typ"), "{html}");
    }

    #[test]
    fn caret_keys_stay_in_the_source_while_ctrl_chords_escape_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);

        // arrows move the caret, not the month grid below; without an
        // injected probe the vertical ones slide nowhere either
        press(&mut dom, keys, Key::ArrowLeft, Modifiers::empty());
        press(&mut dom, keys, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("july 2026"), "no paging: {html}");
        assert!(html.contains("= 2026-07-23"), "no slide: {html}");
        // enter in the source must not reach the create handler either
        press(&mut dom, keys, Key::Enter, Modifiers::empty());
        assert!(html.contains("block-active"), "still editing: {html}");

        // the theme chord still bubbles to the app root
        press(
            &mut dom,
            keys,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="light""#), "{html}");
    }

    #[test]
    fn switching_blocks_flushes_and_moves_the_source() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let bounced = click_for_mutations(&mut dom, clicks[BLOCK_PREAMBLE]);
        // the woken preamble widget has no click listener, so the heading
        // fragment's is the bounce's only one
        let heading = listeners(&bounced, "click")[0];
        let woken = click_for_mutations(&mut dom, heading);
        let sink = listeners(&woken, "keydown")[0];
        // the preamble re-rendered as a fragment under a fresh id
        let preamble = listeners(&woken, "click")[0];
        retype(&mut dom, sink, "= renamed\n");

        let mutations = click_for_mutations(&mut dom, preamble);
        assert_eq!(
            listeners(&mutations, "keydown").len(),
            1,
            "the source moved to the preamble block"
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "the preamble source: {html}");
        let saved =
            std::fs::read_to_string(vault.path().join("time/2026-07-23.typ"))
                .expect("the note is readable");
        assert!(saved.contains("= renamed"), "the move flushed: {saved}");
    }

    #[test]
    fn boundary_arrows_slide_the_source_between_blocks() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);

        // caret on the heading's first line: up slides into the preamble
        place_caret(&mut dom, block, &hit, 0);
        press(&mut dom, keys, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "the preamble source: {html}");
        assert!(!html.contains("= 2026-07-23"), "one active block: {html}");
    }

    #[test]
    fn a_mid_block_caret_slides_nowhere_and_a_missed_press_lands_at_the_end() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_block(&mut dom, clicks[BLOCK_PREAMBLE]);

        // a caret with newlines on both sides is ordinary movement
        place_caret(
            &mut dom,
            block,
            &hit,
            "#import \"/templates/template.typ\": *\n#s".len(),
        );
        press(&mut dom, keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, keys, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "still the preamble: {html}");

        // a press the probe cannot place — the empty margin — lands the
        // caret at the block's end rather than dropping the press
        *hit.lock().expect("the hit cell never poisons") = None;
        mouse(&mut dom, "mousedown", block, (0.0, 0.0));
        block_on(settle(&mut dom));
        press(&mut dom, keys, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "still the preamble: {html}");
    }

    #[test]
    fn a_dead_key_composes_and_commits_at_the_caret() {
        // the spike's transcript, replayed: keydown Dead, composition
        // start/update, the commit keystroke flagged composing, an empty
        // compositionend, then the real one (adr/2026-08-hidden-ime-sink.md)
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);

        press(&mut dom, sink, Key::Dead, Modifiers::empty());
        compose(&mut dom, sink, "compositionstart", "");
        compose(&mut dom, sink, "compositionupdate", "^");
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="compose""#),
            "the dead key previews at the caret: {html}"
        );
        assert_eq!(
            source_of(&dom),
            before,
            "the preview is not in the buffer"
        );

        press_composing(&mut dom, sink, Key::Character("e".into()));
        compose(&mut dom, sink, "compositionend", "");
        assert_eq!(
            source_of(&dom),
            before,
            "the empty end WebKitGTK fires first commits nothing"
        );
        compose(&mut dom, sink, "compositionend", "ê");
        let source = source_of(&dom);
        assert_eq!(
            source,
            format!("{before}ê"),
            "one ê at the caret — never a stray e or ^"
        );
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="compose""#),
            "the preview is gone"
        );
    }

    #[test]
    fn a_keystroke_during_an_open_preview_is_the_imes() {
        // GTK's ordering is not trusted: whatever isComposing says, an
        // open preview means the IME owns the keys (the spike saw both)
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);

        compose(&mut dom, sink, "compositionstart", "");
        compose(&mut dom, sink, "compositionupdate", "¨");
        press(
            &mut dom,
            sink,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert_eq!(source_of(&dom), before, "the x never typed");
        compose(&mut dom, sink, "compositionend", "ï");
        assert_eq!(source_of(&dom), format!("{before}ï"));
    }

    #[test]
    fn shift_arrows_draw_a_selection_and_typing_replaces_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // from the empty last line, shift+up selects the link line
        press(&mut dom, sink, Key::ArrowUp, Modifiers::SHIFT);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="sel""#),
            "the selection is drawn: {html}"
        );

        press(
            &mut dom,
            sink,
            Key::Character("X".into()),
            Modifiers::empty(),
        );
        assert_eq!(
            source_of(&dom),
            "= 2026-07-23\nX",
            "the keystroke replaced the selection"
        );
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="sel""#),
            "and collapsed it"
        );

        // the erase keys answer; tab is consumed inert
        press(&mut dom, sink, Key::Tab, Modifiers::empty());
        press(&mut dom, sink, Key::Delete, Modifiers::empty());
        assert_eq!(
            source_of(&dom),
            "= 2026-07-23\nX",
            "nothing to their right"
        );
        press(&mut dom, sink, Key::Backspace, Modifiers::CONTROL);
        assert_eq!(source_of(&dom), "= 2026-07-23\n", "the word went");
        press(&mut dom, sink, Key::Backspace, Modifiers::empty());
        assert_eq!(source_of(&dom), "= 2026-07-23", "one cluster went");
    }

    #[test]
    fn the_clipboard_chords_round_trip_through_the_seams() {
        let vault = temp_vault();
        let (mut dom, clicks, written) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Some("collé".to_string()),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // copy with nothing selected is a no-op, like the textarea's
        press(
            &mut dom,
            sink,
            Key::Character("c".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert!(written.lock().expect("the write cell").is_empty());

        // select all, copy: the block's whole content reaches the seam
        press(
            &mut dom,
            sink,
            Key::Character("a".into()),
            Modifiers::CONTROL,
        );
        press(
            &mut dom,
            sink,
            Key::Character("c".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert_eq!(
            *written.lock().expect("the write cell"),
            vec!["= 2026-07-23\n#l(\"2026-07-22\")\n".to_string()],
        );

        // cut: a second copy lands and the block empties
        press(
            &mut dom,
            sink,
            Key::Character("x".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert_eq!(written.lock().expect("the write cell").len(), 2);
        assert_eq!(source_of(&dom), "", "the selection went with the cut");

        // cut with nothing selected is a no-op too
        press(
            &mut dom,
            sink,
            Key::Character("x".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert_eq!(written.lock().expect("the write cell").len(), 2);

        // paste inserts what the read seam answers
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), "collé");
    }

    #[test]
    fn clipboard_chords_without_seams_quietly_decline() {
        // headless without fakes: the selection stays, nothing pastes
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);

        press(
            &mut dom,
            sink,
            Key::Character("a".into()),
            Modifiers::CONTROL,
        );
        press(
            &mut dom,
            sink,
            Key::Character("c".into()),
            Modifiers::CONTROL,
        );
        press(
            &mut dom,
            sink,
            Key::Character("x".into()),
            Modifiers::CONTROL,
        );
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before, "nothing moved without seams");

        // a paste whose read answers nothing pastes nothing
        let (mut dom, clicks, _) =
            clipboard_app(Some(vault.path().to_path_buf()), None);
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before);
    }

    #[test]
    fn a_drag_extends_the_selection_one_probe_in_flight_at_a_time() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, _) = activate_heading(&mut dom, &clicks);

        // the press anchors at the start…
        *hit.lock().expect("the hit cell never poisons") = Some((0, 0));
        mouse(&mut dom, "mousedown", block, (0.0, 0.0));
        block_on(settle(&mut dom));

        // …the drag moves the head; a second move while the first probe is
        // out is skipped rather than queued
        *hit.lock().expect("the hit cell never poisons") = Some((0, 5));
        mouse(&mut dom, "mousemove", block, (40.0, 0.0));
        mouse(&mut dom, "mousemove", block, (41.0, 0.0));
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"<span class="sel" data-start="0">= 202</span>"#),
            "the drag drew the selection: {html}"
        );

        // a move whose probe misses extends nothing
        *hit.lock().expect("the hit cell never poisons") = None;
        mouse(&mut dom, "mousemove", block, (60.0, 0.0));
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom)
                .contains(r#"<span class="sel" data-start="0">= 202</span>"#),
            "the miss changed nothing"
        );

        // after the release, moves stop extending
        mouse(&mut dom, "mouseup", block, (41.0, 0.0));
        *hit.lock().expect("the hit cell never poisons") = Some((0, 9));
        mouse(&mut dom, "mousemove", block, (80.0, 0.0));
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom)
                .contains(r#"<span class="sel" data-start="0">= 202</span>"#),
            "the selection held where the button went up"
        );

        // one probe in flight: while one hangs, the next move is skipped
        // rather than queued
        mouse(&mut dom, "mousedown", block, (0.0, 0.0));
        *hit.lock().expect("the hit cell never poisons") = Some(HIT_HANGS);
        mouse(&mut dom, "mousemove", block, (90.0, 0.0));
        *hit.lock().expect("the hit cell never poisons") = Some((0, 9));
        mouse(&mut dom, "mousemove", block, (91.0, 0.0));
        block_on(settle(&mut dom));
        assert!(
            !dioxus_ssr::render(&dom)
                .contains(r#"<span class="sel" data-start="0">= 2026-07"#),
            "the second move was skipped while the first probe was out"
        );
    }

    #[test]
    fn presses_without_a_hit_probe_keep_the_caret_and_ctrl_still_follows() {
        // headless without a fake: the caret holds; with Ctrl the press
        // still follows whatever the caret already stands in
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (block, _) = activate_heading(&mut dom, &clicks);

        mouse(&mut dom, "mousedown", block, (0.0, 0.0));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "{html}");
        // a drag without a probe extends nothing either
        mouse(&mut dom, "mousemove", block, (10.0, 0.0));
        assert!(!dioxus_ssr::render(&dom).contains(r#"class="sel""#));

        // the caret opens on the empty last line: nothing to follow there
        ctrl_press(&mut dom, block);
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "no link under the caret, nothing followed: {html}"
        );
    }

    #[test]
    fn the_sink_takes_focus_back_when_an_overlay_closes() {
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let keys = listeners(&mutations, "keydown");
        // registration order: the pane, then the sink, then the caret span
        // — the sink is the one that also holds a keydown listener
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[1],
        );
        block_on(settle(&mut dom));
        let before = focused.load(Ordering::SeqCst);

        // the picker's input owns the focus while it is up; closing hands
        // it back to the sink through the focus effect
        let (_, picker_keys) = open_picker(&mut dom, keys[LOGS_KEYS]);
        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        block_on(settle(&mut dom));
        assert!(
            focused.load(Ordering::SeqCst) > before,
            "the sink asked for focus once the picker closed"
        );
    }

    // -- the scale chain jumps -----------------------------------------------

    #[test]
    fn a_breadcrumb_click_swaps_the_centre_to_the_wider_scale() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CRUMB_WEEK]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">weekly<"), "{html}");
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    // -- the jump panel: existence marking, paging, month sync ---------------

    #[test]
    fn the_grid_marks_existing_days_and_the_selected_pill() {
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert_eq!(
            html.matches("has-note").count(),
            3,
            "the three july days: {html}"
        );
        assert!(html.contains("cal-day has-note selected"), "{html}");
        assert!(html.contains("july 2026"), "{html}");
    }

    #[test]
    fn a_selected_empty_day_outlines_without_a_note_marker() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(24)]);
        let html = dioxus_ssr::render(&dom);
        // the double space is the unfired has-note conditional class slot
        assert!(html.contains(r#"class="cal-day  selected""#), "{html}");
    }

    #[test]
    fn the_wheel_pages_months_and_ignores_a_zero_delta() {
        let vault = temp_vault();
        let (mut dom, _, _, wheels) =
            rendered_app(Some(vault.path().to_path_buf()));
        let wheel = wheels[0];

        scroll(&mut dom, wheel, -120.0);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("june 2026"), "up pages back: {html}");
        assert!(!html.contains("has-note"), "june holds no notes: {html}");

        scroll(&mut dom, wheel, 120.0);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("july 2026"), "down pages forward: {html}");

        scroll(&mut dom, wheel, 0.0);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("july 2026"), "zero pages nowhere: {html}");
    }

    #[test]
    fn selecting_across_scales_moves_the_grid_month() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_SUMMER]);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("may 2026"),
            "the season starts in may: {html}"
        );
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn the_current_season_is_lit_in_the_season_row() {
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-season lit"), "{html}");
        assert_eq!(html.matches("cal-season lit").count(), 1, "{html}");
    }

    #[test]
    fn clicking_a_rail_week_selects_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_W30]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">weekly<"), "{html}");
        assert!(html.contains("cal-week selected"), "{html}");
    }

    // -- the header controls: ‹ today › and their keyboard twins -------------

    #[test]
    fn the_header_chevrons_page_the_month_both_ways() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CAL_BACK]);
        click(&mut dom, clicks[CAL_BACK]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("may 2026"), "‹ pages back: {html}");
        click(&mut dom, clicks[CAL_FORWARD]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("june 2026"), "› pages forward: {html}");
        // paging moves the grid only; the selection stays on today
        assert!(html.contains(">2026-07-23<"), "{html}");
    }

    #[test]
    fn the_today_button_returns_to_today_without_writing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // wander: select the empty day, then page the grid away
        click(&mut dom, clicks[day_cell(24)]);
        click(&mut dom, clicks[CAL_BACK]);
        click(&mut dom, clicks[CAL_TODAY]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">2026-07-23<"), "today is selected: {html}");
        assert!(html.contains(RENDERED_NOTE), "{html}");
        assert!(html.contains("july 2026"), "the month snaps back: {html}");
        assert!(
            !vault.path().join("time/2026-07-24.typ").exists(),
            "navigating never writes"
        );
    }

    #[test]
    fn arrow_keys_page_the_month_like_the_chevrons() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::ArrowLeft,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("june 2026"), "left pages back: {html}");
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::ArrowRight,
            Modifiers::empty(),
        );
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::ArrowRight,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("august 2026"), "right pages forward: {html}");
        // the grid moved, the selection did not
        assert!(html.contains(">2026-07-23<"), "{html}");
    }

    // -- load_notes: the error edges behind the vault-error screen -----------

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
    fn a_sabotaged_notes_table_fails_the_survey_and_the_count() {
        let vault = temp_vault();
        let index = sabotaged_index(vault.path(), "DROP TABLE notes");
        let error = survey(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
        let error = open_loops(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_sabotaged_links_table_fails_the_survey_count() {
        // the time-note half survives on the notes table; the count is what
        // reaches the links table and fails
        let vault = temp_vault();
        let index = sabotaged_index(vault.path(), "DROP TABLE links");
        let error = survey(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_missing_created_column_fails_only_the_table_notes() {
        // the one sabotage the earlier survey legs survive: time notes and
        // every loops query still answer, only the table's created is gone
        let vault = temp_vault();
        let index = sabotaged_index(
            vault.path(),
            "ALTER TABLE notes DROP COLUMN created",
        );
        assert!(index.time_notes().is_ok());
        assert!(open_loops(&index).is_ok());
        let error = survey(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_blob_link_source_fails_only_the_link_edges() {
        // the one sabotage every earlier survey leg survives: a typeless
        // time note is invisible to the rail and the table, its path
        // decodes fine for the loops — only link_edges reads the blob id
        let vault = temp_vault();
        let index = sabotaged_index(
            vault.path(),
            "INSERT INTO notes (path, category, id) \
             VALUES ('time/blob.typ', 'time', x'00'); \
             INSERT INTO links (source_path, target_id) \
             VALUES ('time/blob.typ', 'alpha');",
        );
        assert!(index.time_notes().is_ok());
        assert!(open_loops(&index).is_ok());
        assert!(index.table_notes().is_ok());
        let error = survey(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_sabotaged_notes_table_shows_the_captured_error_after_open() {
        // Index::open succeeds (the version stamp survives), the captured
        // query is what fails — the second error edge of captured_lines
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE notes")
            .expect("the sabotage succeeds");
        click(&mut dom, clicks[RAIL_DAY_23]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "the note itself is fine");
        assert!(html.contains("captured today:"), "{html}");
    }

    #[test]
    fn a_missing_summarized_column_fails_the_survey_and_the_captured_block() {
        // the one sabotage the sibling queries survive: typeless notes and
        // dangling links still answer, only the summary column is gone
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let index = sabotaged_index(
            vault.path(),
            "ALTER TABLE notes DROP COLUMN summarized",
        );
        let error = open_loops(&index).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");

        click(&mut dom, clicks[RAIL_DAY_23]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "the note itself is fine");
        assert!(html.contains("captured today:"), "{html}");
    }

    // -- the watcher's batches reach the screen ------------------------------

    /// The app with a vault feed injected, plus the sender the test keeps to
    /// play the watcher itself.
    fn watched_app(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        tokio::sync::mpsc::UnboundedSender<Vec<watch::VaultChange>>,
    ) {
        set_event_converter(Box::new(TestEvents));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(VaultFeed(Arc::new(
            Mutex::new(Some(receiver)),
        ))));
        let mutations = with_reactor(|| dom.rebuild_to_vec());
        let clicks = listeners(&mutations, "click");
        (dom, clicks, sender)
    }

    /// Sends one batch and lets the shell's task run to its next await.
    fn feed_batch(
        dom: &mut VirtualDom,
        sender: &tokio::sync::mpsc::UnboundedSender<Vec<watch::VaultChange>>,
        batch: Vec<watch::VaultChange>,
    ) {
        sender.send(batch).expect("the shell holds the receiver");
        block_on(settle(dom));
    }

    /// Writes an unsummarized capture into the vault, the way the headless
    /// `--capture` process would while the app is open.
    fn write_capture(vault: &Path, id: &str) -> PathBuf {
        let path = vault.join(format!("capture/{id}.typ"));
        std::fs::write(
            &path,
            format!(
                "#import \"/templates/template.typ\": *\n\
                 #show: note\n\
                 #meta(id: \"{id}\", created: \"{TODAY}\")\n\
                 \n== Summary\n\n== Original\n\nvenu du dehors\n"
            ),
        )
        .expect("the capture is written");
        path
    }

    #[test]
    fn a_capture_written_from_outside_reaches_the_ember_and_the_day() {
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        assert!(
            !dioxus_ssr::render(&dom).contains("ember"),
            "the vault opens with no loops"
        );

        let path = write_capture(vault.path(), "capture-du-dehors");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Capture,
                path: PathBuf::from("capture/capture-du-dehors.typ"),
            }],
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="ember">1</span>"#), "{html}");
        assert!(html.contains("capture-du-dehors · still open"), "{html}");

        // and it leaves again when the file does
        std::fs::remove_file(&path).expect("the capture is deleted");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Removed(PathBuf::from(
                "capture/capture-du-dehors.typ",
            ))],
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("ember"), "back to nothing owed: {html}");
    }

    #[test]
    fn a_rescan_batch_rebuilds_the_whole_index() {
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        write_capture(vault.path(), "capture-rescan");
        std::fs::write(
            vault.path().join("time/2026-07-24.typ"),
            time_note("2026-07-24", "daily"),
        )
        .expect("the new day is written");

        feed_batch(&mut dom, &sender, vec![watch::VaultChange::Rescan]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="ember">1</span>"#), "{html}");
        assert!(
            html.contains(r#"<span class="rail-id">2026-07-24</span>"#),
            "the rail caught the new day too: {html}"
        );
    }

    #[test]
    fn a_batch_the_index_cannot_absorb_becomes_the_notice() {
        // an index that will not even open
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join(".index/index.db"))
            .expect("the database is there to remove");
        std::fs::create_dir(vault.path().join(".index/index.db"))
            .expect("a directory squats the database path");
        feed_batch(&mut dom, &sender, vec![watch::VaultChange::Rescan]);
        assert!(dioxus_ssr::render(&dom).contains("watching the vault"));
    }

    #[test]
    fn a_change_that_cannot_be_read_becomes_the_notice() {
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        // a directory where the watcher says a note is: not missing, which
        // would be a deletion, but unreadable
        std::fs::create_dir(vault.path().join("capture/impossible.typ"))
            .expect("the fake note is created");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Capture,
                path: PathBuf::from("capture/impossible.typ"),
            }],
        );
        assert!(dioxus_ssr::render(&dom).contains("watching the vault"));
    }

    #[test]
    fn a_reread_that_fails_after_the_change_lands_becomes_the_notice() {
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE links")
            .expect("the sabotage succeeds");
        // an empty batch changes nothing, so the failure can only be the
        // re-read the screen is refreshed from
        feed_batch(&mut dom, &sender, vec![]);
        assert!(dioxus_ssr::render(&dom).contains("watching the vault"));
    }

    #[test]
    fn a_watcher_that_stops_ends_the_task_rather_than_spinning() {
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        drop(sender);
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom).contains("rail-id"),
            "the screen stands, it just stops hearing about the vault"
        );
    }

    #[test]
    fn an_app_with_no_feed_keeps_the_index_it_launched_with() {
        // every other test mounts this way; the shell must simply not watch
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        assert!(!dioxus_ssr::render(&dom).contains("watching the vault"));
    }

    #[test]
    fn a_feed_already_taken_starts_no_second_watcher() {
        let vault = temp_vault();
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(Some(
            vault.path().to_path_buf(),
        ))));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        // the cell arrives empty, as it would on a second shell
        dom.insert_any_root_context(Box::new(VaultFeed(Arc::new(
            Mutex::new(None),
        ))));
        with_reactor(|| dom.rebuild_to_vec());
        assert!(
            dioxus_ssr::render(&dom).contains("rail-id"),
            "it still renders"
        );
    }

    // -- in-app capture: the clipboard becomes a note ------------------------

    /// The capture clock, on the same day as `TODAY` so the new note lands
    /// in that day's "captured today" block.
    const CAPTURED_AT: &str = "2026-07-23T09:15:42+02:00[Europe/Paris]";

    fn capture_chord(dom: &mut VirtualDom, keys: &[ElementId]) {
        press(
            dom,
            keys[LOGS_KEYS],
            Key::Character("V".to_string()),
            Modifiers::CONTROL | Modifiers::SHIFT,
        );
    }

    #[test]
    fn the_capture_chord_writes_what_is_on_the_clipboard() {
        let vault = temp_vault();
        let (mut dom, _, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Some("collé du navigateur".to_string()),
            Some(CAPTURED_AT),
        );
        capture_chord(&mut dom, &keys);

        let written =
            vault.path().join("capture/capture-2026-07-23-091542.typ");
        let text = std::fs::read_to_string(&written).expect("the capture");
        assert!(text.contains("collé du navigateur"), "{text}");
        assert!(text.contains(r#"created: "2026-07-23""#), "{text}");
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("captured capture-2026-07-23-091542"),
            "{html}"
        );
    }

    #[test]
    fn a_capture_that_cannot_be_written_says_so() {
        // the same second twice: the second one's id is taken
        let vault = temp_vault();
        let (mut dom, _, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Some("deux fois".to_string()),
            Some(CAPTURED_AT),
        );
        capture_chord(&mut dom, &keys);
        capture_chord(&mut dom, &keys);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("capture: AlreadyExists"), "{html}");
    }

    #[test]
    fn the_capture_chord_needs_a_clipboard_that_answers_and_a_clock() {
        let vault = temp_vault();
        let captures = vault.path().join("capture");
        let count = || {
            std::fs::read_dir(&captures)
                .expect("the capture directory is there")
                .count()
        };
        let before = count();

        // a clipboard that answers nothing captures nothing
        let (mut dom, _, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            None,
            Some(CAPTURED_AT),
        );
        capture_chord(&mut dom, &keys);
        assert_eq!(count(), before);

        // nor does one with no clock to stamp the note by
        let (mut dom, _, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Some("sans horloge".to_string()),
            None,
        );
        capture_chord(&mut dom, &keys);
        assert_eq!(count(), before);

        // and with no clipboard injected at all the chord is inert
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        capture_chord(&mut dom, &keys);
        assert_eq!(count(), before);
    }

    #[test]
    fn over_an_active_block_the_chord_still_captures() {
        // a block is now always active over an open note: the chord
        // suppresses the webview's paste and captures — one keystroke,
        // one action (adr/2026-08-cursor-always-in-the-note.md)
        let vault = temp_vault();
        let captures = vault.path().join("capture");
        let before = std::fs::read_dir(&captures)
            .expect("the capture directory is there")
            .count();
        let (mut dom, clicks, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Some("pour la capture".to_string()),
            Some(CAPTURED_AT),
        );
        activate_heading(&mut dom, &clicks);
        capture_chord(&mut dom, &keys);
        assert_eq!(
            std::fs::read_dir(&captures)
                .expect("the capture directory is there")
                .count(),
            before + 1
        );
    }

    // -- the link picker: Ctrl+L, filter, accept -----------------------------

    #[test]
    fn ctrl_l_opens_the_picker_and_enter_writes_the_link() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);

        // the caret sits right after "= 2026-07-23\n"
        let anchor = "= 2026-07-23\n".len();
        place_caret(&mut dom, block, &hit, anchor);
        let (input, picker_keys) = open_picker(&mut dom, keys);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("link-picker"), "{html}");
        assert!(html.contains("2026-w30"), "the vault is listed: {html}");

        type_into(&mut dom, input, "summer");
        assert_eq!(
            picker_ids(&dom),
            vec!["2026-summer"],
            "the query filters the list"
        );

        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "accepting closes it: {html}");
        assert!(
            source_of(&dom).contains(r#"#l("2026-summer")#l("2026-07-22")"#),
            "spliced at the caret: {}",
            source_of(&dom)
        );
        // the caret lands past the link it just wrote: the drawn caret
        // stands between the new link and the old one
        assert!(
            html.contains(r#"summer&#34;)</span><span class="caret">"#),
            "{html}"
        );
    }

    #[test]
    fn a_row_click_accepts_the_completion_too() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 0);

        let mutations =
            press_for_mutations(&mut dom, keys, ctrl_l(), Modifiers::CONTROL);
        // the picker's own click targets, after the input's listeners
        let rows = listeners(&mutations, "click");
        click(&mut dom, rows[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");
        assert!(
            source_of(&dom).starts_with(r#"#l("2026-07-21")"#),
            "the first row went in at the caret: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn the_arrows_move_the_highlight_and_stop_at_both_ends() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);
        let (input, picker_keys) = open_picker(&mut dom, keys);

        // two entries: the daily notes 22 and 23
        type_into(&mut dom, input, "2026-07-2");
        let selected = |dom: &VirtualDom| {
            let html = dioxus_ssr::render(dom);
            html.split("picker-row selected")
                .nth(1)
                .and_then(|rest| rest.split("picker-id\">").nth(1))
                .and_then(|rest| rest.split('<').next())
                .map(str::to_string)
                .unwrap_or_else(|| panic!("no highlighted row: {html}"))
        };
        assert_eq!(selected(&dom), "2026-07-21", "the first row starts lit");

        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        assert_eq!(selected(&dom), "2026-07-22");
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        assert_eq!(selected(&dom), "2026-07-23", "the last row holds");

        press(&mut dom, picker_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowUp, Modifiers::empty());
        assert_eq!(selected(&dom), "2026-07-21", "and the first one holds");
    }

    #[test]
    fn escape_closes_the_picker_and_a_query_that_matches_nothing_writes_nothing()
     {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);
        let (input, picker_keys) = open_picker(&mut dom, keys);

        type_into(&mut dom, input, "fantôme");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching note"), "{html}");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("link-picker"),
            "enter matched nothing: {html}"
        );
        assert!(!source_of(&dom).contains("fant"), "and wrote nothing");

        // an unhandled key inside the picker is absorbed, not acted on
        press(
            &mut dom,
            picker_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "escape closes it: {html}");
        assert!(
            html.contains("= 2026-07-23"),
            "the source is intact: {html}"
        );
    }

    #[test]
    fn escaping_the_picker_leaves_the_caret_where_it_stood() {
        // the caret is app state: the picker's input held the focus, but
        // nothing could move the caret — escape just closes the overlay
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, LINK_IN_HEADING);
        let (_, picker_keys) = open_picker(&mut dom, keys);

        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");
        assert!(html.contains("block-active"), "still editing: {html}");
        // the caret still stands where Ctrl+L found it: at the start of
        // the heading's second line, before the day link
        assert!(
            html.contains(
                r#"<div class="source-line"><span class="caret"></span><span data-start="13">"#
            ),
            "{html}"
        );
    }

    #[test]
    fn the_theme_chord_still_works_over_an_open_picker() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);
        let (_, picker_keys) = open_picker(&mut dom, keys);

        press(
            &mut dom,
            picker_keys,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="light""#), "{html}");
        assert!(html.contains("link-picker"), "and it stays open: {html}");
    }

    #[test]
    fn ctrl_l_needs_an_active_block_and_a_closed_picker() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        // an empty day holds a closed editor: no block, no caret to
        // anchor to, no picker
        click(&mut dom, clicks[day_cell(20)]);
        press(&mut dom, keys[LOGS_KEYS], ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");

        // back over a note: open, then a second Ctrl+L leaves the first
        // one alone
        click(&mut dom, clicks[RAIL_DAY_23]);
        let (input, _) = open_picker(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "summer");
        press(&mut dom, keys[LOGS_KEYS], ctrl_l(), Modifiers::CONTROL);
        assert_eq!(
            picker_ids(&dom),
            vec!["2026-summer"],
            "the second chord left the open picker alone"
        );
    }

    // -- Ctrl+Enter: following the link under the caret ---------------------

    /// The heading block of the fixture's selected day is
    /// `= 2026-07-23\n#l("2026-07-22")`, so the link opens at this offset.
    const LINK_IN_HEADING: usize = "= 2026-07-23\n".len();

    #[test]
    fn ctrl_enter_opens_the_time_note_under_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);

        // inside the `#l("2026-07-22")` the heading block ends with
        place_caret(&mut dom, block, &hit, LINK_IN_HEADING + 3);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the chord jumped to the linked day: {html}"
        );
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn ctrl_pressing_a_link_in_the_source_opens_it_too() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, _) = activate_heading(&mut dom, &clicks);

        // the press asks the probe where it landed: inside the day link
        *hit.lock().expect("the hit cell never poisons") =
            Some((0, LINK_IN_HEADING + 3));
        ctrl_press(&mut dom, block);
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the ctrl+press jumped to the linked day: {html}"
        );
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn a_plain_press_in_the_source_only_moves_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, _) = activate_heading(&mut dom, &clicks);

        // the caret lands in the link, but without the modifier nothing
        // follows
        place_caret(&mut dom, block, &hit, LINK_IN_HEADING + 3);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "the selection stayed put: {html}"
        );
        assert!(html.contains("block-active"), "still editing: {html}");
    }

    #[test]
    fn ctrl_enter_away_from_a_link_neither_jumps_nor_creates() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);

        // in the heading text, well before the link
        place_caret(&mut dom, block, &hit, 2);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "the selection stayed put: {html}"
        );
    }

    #[test]
    fn ctrl_enter_without_an_active_block_writes_no_file() {
        let vault = temp_vault();
        // an empty day is selected, the one the plain Enter would create
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(20)]);
        let (_, keys, _) = pane_targets(&mut dom, &clicks);

        // a closed editor has no caret to follow from
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        assert!(
            !vault.path().join("time/2026-07-20.typ").exists(),
            "the chord is not the create keystroke"
        );
    }

    #[test]
    fn an_index_that_cannot_list_notes_becomes_the_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE notes")
            .expect("the sabotage succeeds");

        press(&mut dom, keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");
        assert!(html.contains("links:"), "{html}");
    }

    #[test]
    fn a_picker_over_an_unopenable_index_says_so() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);
        replace_database_with_a_directory(vault.path());

        press(&mut dom, keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("links:"), "{html}");
    }

    // -- the links footer: both directions, dangling marked ------------------

    #[test]
    fn the_footer_shows_both_directions_and_jumps_where_it_can() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("links-footer"), "{html}");
        assert!(html.contains('←') && html.contains('→'), "{html}");
        assert_eq!(
            html.matches("link-entry link-jump").count(),
            2,
            "both directions reach 2026-07-22: {html}"
        );

        click(&mut dom, clicks[FOOTER_OUTGOING]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">22"), "{html}");
        assert!(html.contains(RENDERED_NOTE), "{html}");

        // and the other direction jumps the same way, from the day it
        // landed on back to the one that links to it
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[FOOTER_BACKLINK]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">22"), "{html}");
    }

    #[test]
    fn a_permanent_backlink_opens_its_sheet_from_the_logs() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/alpha.typ"),
            linking(note("alpha"), "2026-07-23"),
        )
        .expect("alpha is rewritten");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="link-entry link-jump">alpha"#),
            "the v0 inert entry is now a jump: {html}"
        );

        // backlinks order by path, so alpha's entry registers first
        click(&mut dom, clicks[FOOTER_BACKLINK]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="table""#), "{html}");
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains("raised"), "the origin card is lit: {html}");
    }

    #[test]
    fn an_id_less_backlink_stays_inert() {
        let vault = temp_vault();
        // a note with no id links today: its backlink is labelled by its
        // stem, which no card can host — visible, never clickable
        std::fs::write(
            vault.path().join("permanent/anonyme.typ"),
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             \n= anonyme\n#l(\"2026-07-23\")\n",
        )
        .expect("the id-less note is written");
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"<span class="link-entry ">anonyme</span>"#),
            "no jump, no dangling mark: {html}"
        );
    }

    #[test]
    fn a_link_to_nothing_is_marked_as_it_is_typed() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        // the heading block loses its outgoing link and gains a ghost one
        retype(&mut dom, sink, "= 2026-07-23\n#l(\"fantôme\")\n");
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("link-dangling\">fantôme"), "{html}");
        assert_eq!(
            html.matches("link-entry link-jump").count(),
            1,
            "only the backlink stays clickable: {html}"
        );
    }

    #[test]
    fn a_direction_with_nothing_in_it_renders_no_row() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        retype(&mut dom, sink, "= 2026-07-23\n");
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains('←'), "the backlink survives: {html}");
        assert!(!html.contains('→'), "the outgoing row is gone: {html}");
    }

    #[test]
    fn a_note_with_no_links_and_an_empty_day_carry_no_footer() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        // 2026-07-21 links nowhere and nothing links to it
        click(&mut dom, clicks[RAIL_DAY_21]);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("links-footer"), "{html}");

        // and an empty day has no note to have links at all
        click(&mut dom, clicks[day_cell(24)]);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("links-footer"), "{html}");
    }

    #[test]
    fn a_database_that_will_not_answer_surfaces_in_the_footer() {
        for sabotage in ["DROP TABLE notes", "DROP TABLE links"] {
            let vault = temp_vault();
            let (mut dom, clicks, _, _) =
                rendered_app(Some(vault.path().to_path_buf()));
            let saboteur = rusqlite::Connection::open(
                vault.path().join(".index/index.db"),
            )
            .expect("a second connection opens");
            saboteur
                .execute_batch(sabotage)
                .expect("the sabotage succeeds");
            click(&mut dom, clicks[RAIL_DAY_23]);
            let html = dioxus_ssr::render(&dom);
            assert!(html.contains("links:"), "after {sabotage}: {html}");
        }
    }

    #[test]
    fn a_footer_over_an_unopenable_index_says_so() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        replace_database_with_a_directory(vault.path());
        click(&mut dom, clicks[RAIL_DAY_23]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("links:"), "{html}");
    }

    // -- the command palette: every command reachable by name ---------------

    #[test]
    fn ctrl_p_opens_the_palette_and_typing_filters() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, _) = open_palette(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("command-palette"), "{html}");
        assert!(html.contains(">commands<"), "the head names it: {html}");
        assert!(html.contains("ctrl+shift+v"), "the chords show: {html}");
        assert_eq!(
            palette_labels(&dom),
            vec![
                "toggle theme",
                "quit",
                "capture clipboard",
                "insert link",
                "follow link",
                "previous month",
                "next month",
                "open loops",
                "go to today",
                "go to table",
                "new note",
            ],
            "the note opened editing, so the caret commands stand; the \
             screen already stood on is not offered, and no sheet backs \
             a delete"
        );

        type_into(&mut dom, input, "THEME");
        assert_eq!(
            palette_labels(&dom),
            vec!["toggle theme"],
            "the query filters, ignoring case"
        );
    }

    #[test]
    fn over_an_active_block_all_but_the_stood_screen_are_listed() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);
        open_palette(&mut dom, keys);
        let labels = palette_labels(&dom);
        assert_eq!(labels.len(), 11, "{labels:?}");
        assert!(labels.contains(&"insert link".to_string()), "{labels:?}");
        assert!(labels.contains(&"follow link".to_string()), "{labels:?}");
    }

    #[test]
    fn the_palette_runs_toggle_theme_and_closes() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "theme");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="light""#), "{html}");
        assert!(!html.contains("command-palette"), "and it closed: {html}");
    }

    #[test]
    fn a_row_click_runs_the_command_too() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let mutations = press_for_mutations(
            &mut dom,
            keys[LOGS_KEYS],
            ctrl_p(),
            Modifiers::CONTROL,
        );
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        // the first row is `toggle theme`, the registry's order
        click(&mut dom, listeners(&mutations, "click")[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="light""#), "{html}");
        assert!(!html.contains("command-palette"), "{html}");
    }

    #[test]
    fn the_palette_runs_quit() {
        let vault = temp_vault();
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let recorder = closed.clone();
        let closer = Closer(Arc::new(move || {
            recorder.store(true, Ordering::SeqCst);
        }));
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), Some(closer));
        let keys = listeners(&mutations, "keydown")[LOGS_KEYS];
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "quit");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(closed.load(Ordering::SeqCst));
    }

    #[test]
    fn the_palette_runs_capture_clipboard() {
        let vault = temp_vault();
        let (mut dom, _, keydowns) = capture_app(
            Some(vault.path().to_path_buf()),
            Some("pris du web".to_string()),
            Some(CAPTURED_AT),
        );
        let (input, palette_keys) =
            open_palette(&mut dom, keydowns[LOGS_KEYS]);
        type_into(&mut dom, input, "capture");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("captured capture-"), "{html}");
    }

    #[test]
    fn the_palette_runs_insert_link_at_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);
        // the caret is app state: the palette cannot move it, so the link
        // lands where it stood before Ctrl+P
        place_caret(&mut dom, block, &hit, LINK_IN_HEADING);
        let (input, palette_keys) = open_palette(&mut dom, keys);

        type_into(&mut dom, input, "insert");
        let mutations = press_for_mutations(
            &mut dom,
            palette_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("link-picker"), "{html}");
        assert!(!html.contains("command-palette"), "{html}");

        // the picker works exactly as if Ctrl+L had opened it
        let picker_input = listeners(&mutations, "input")[0];
        let picker_keys = listeners(&mutations, "keydown")[0];
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        type_into(&mut dom, picker_input, "summer");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        assert!(
            source_of(&dom).contains(r#"#l("2026-summer")#l("2026-07-22")"#),
            "spliced at the caret: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn the_palette_runs_follow_link_from_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, LINK_IN_HEADING + 3);
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "follow");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the command jumped to the linked day: {html}"
        );
    }

    #[test]
    fn the_palette_pages_the_month_both_ways() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "previous");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("june 2026"), "{html}");

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "next");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("july 2026"), "{html}");
    }

    #[test]
    fn the_palette_toggles_the_loops_list() {
        let vault = debt_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "loops");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("loops-list"), "{html}");

        // the same command is the way back
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "loops");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("loops-list"), "{html}");
    }

    #[test]
    fn the_palette_goes_back_to_today() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">21"), "{html}");

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "today");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">23"), "{html}");
    }

    #[test]
    fn escape_closes_the_palette_and_the_pane_takes_focus_back() {
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let keys = listeners(&mutations, "keydown")[LOGS_KEYS];
        let clicks = listeners(&mutations, "click");
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );
        // an empty selection: the editor closes, the pane holds the chords
        // — the one state where no textarea competes for focus
        click(&mut dom, clicks[day_cell(24)]);

        let (_, palette_keys) = open_palette(&mut dom, keys);
        block_on(settle(&mut dom));
        // while the palette is up, the pane leaves focus to its input
        let up = focused.load(Ordering::SeqCst);

        press(&mut dom, palette_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("command-palette"),
            "escape closed it: {html}"
        );
        block_on(settle(&mut dom));
        assert!(
            focused.load(Ordering::SeqCst) > up,
            "the pane asked for focus once the palette closed"
        );
    }

    #[test]
    fn escape_over_a_block_leaves_the_caret_where_the_palette_found_it() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, LINK_IN_HEADING);

        let (_, palette_keys) = open_palette(&mut dom, keys);
        press(&mut dom, palette_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "still editing: {html}");
        // the caret is app state the palette never touched
        assert!(
            html.contains(
                r#"<div class="source-line"><span class="caret"></span><span data-start="13">"#
            ),
            "{html}"
        );
    }

    #[test]
    fn enter_over_no_matching_command_does_nothing() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "xyzzy");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching command"), "{html}");

        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("command-palette"),
            "enter matched nothing: {html}"
        );
        // an unhandled key inside the palette is absorbed, not acted on
        press(
            &mut dom,
            palette_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("command-palette"), "{html}");
    }

    #[test]
    fn the_palette_arrows_move_the_highlight_and_stop_at_both_ends() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        // two rows: previous month, next month
        type_into(&mut dom, input, "month");
        let selected = |dom: &VirtualDom| {
            let html = dioxus_ssr::render(dom);
            html.split("palette-row selected")
                .nth(1)
                .and_then(|rest| rest.split(r#"palette-label">"#).nth(1))
                .and_then(|rest| rest.split('<').next())
                .map(str::to_string)
                .unwrap_or_else(|| panic!("no highlighted row: {html}"))
        };
        assert_eq!(selected(&dom), "previous month", "the first row is lit");

        press(&mut dom, palette_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, palette_keys, Key::ArrowDown, Modifiers::empty());
        assert_eq!(selected(&dom), "next month", "the last row holds");

        press(&mut dom, palette_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, palette_keys, Key::ArrowUp, Modifiers::empty());
        assert_eq!(selected(&dom), "previous month", "the first one holds");
    }

    #[test]
    fn ctrl_p_guards_and_the_chords_still_bubble_over_it() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "theme");

        // a second Ctrl+P leaves the open palette alone
        press(&mut dom, keys[LOGS_KEYS], ctrl_p(), Modifiers::CONTROL);
        assert_eq!(
            palette_labels(&dom),
            vec!["toggle theme"],
            "the second chord left the open palette alone"
        );

        // the theme chord works over the open palette, which stays up
        press(
            &mut dom,
            palette_keys,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="light""#), "{html}");
        assert!(html.contains("command-palette"), "still open: {html}");

        // and over an open link picker, Ctrl+P is inert
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, block_keys) = activate_heading(&mut dom, &clicks);
        open_picker(&mut dom, block_keys);
        press(&mut dom, block_keys, ctrl_p(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command-palette"), "{html}");
        assert!(html.contains("link-picker"), "{html}");
    }

    // -- auto-placement and the explicit arrange ------------------------------

    /// A vault whose alpha stands hand-placed at (1000, 500), with beta
    /// linking it — the auto-placement scenario.
    fn anchored_vault() -> tempfile::TempDir {
        let vault = temp_vault();
        std::fs::create_dir_all(vault.path().join(".index"))
            .expect("the index dir is creatable");
        std::fs::write(
            vault.path().join(".index/positions"),
            "alpha 1000 500\n",
        )
        .expect("the hand placement is written");
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            linking(note("beta"), "alpha"),
        )
        .expect("the linking note is written");
        vault
    }

    #[test]
    fn a_linked_unplaced_note_appears_beside_its_anchor() {
        let vault = anchored_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("left: 1000px; top: 500px"), "{html}");
        // beta proposes the first ring cell beside its anchor
        assert!(html.contains("left: 808px; top: 404px"), "{html}");
    }

    #[test]
    fn auto_place_never_writes_the_positions_file() {
        let vault = anchored_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        assert!(dioxus_ssr::render(&dom).contains("left: 808px; top: 404px"));
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert_eq!(
            saved.trim(),
            "alpha 1000 500",
            "the proposal stayed a proposal — the invariant is structural"
        );
    }

    #[test]
    fn arrange_cluster_moves_the_component_and_the_debounce_persists_it() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            linking(note("beta"), "alpha"),
        )
        .expect("the linking note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "arrange");
        assert_eq!(palette_labels(&dom), vec!["arrange cluster"]);
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert!(saved.contains("alpha "), "the anchor arranged: {saved}");
        assert!(saved.contains("beta "), "its neighbour too: {saved}");
        assert_eq!(
            saved.trim().lines().count(),
            2,
            "cards outside the component were never touched: {saved}"
        );
    }

    #[test]
    fn an_arrange_whose_sheet_left_meanwhile_moves_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        // the delete guard's twin: the palette froze "a sheet is open",
        // the sheet closed under it, the frozen command runs into nothing
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "arrange");
        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the idle tick still writes the empty store");
        assert_eq!(saved.trim(), "", "no cluster to arrange: {saved}");
    }

    // -- findability: the filter and the jump ---------------------------------

    #[test]
    fn ctrl_f_applies_the_highlighted_filter_and_dims_the_rest() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            tagged_note("beta", "method"),
        )
        .expect("the tagged note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (input, filter_keys) = open_overlay(&mut dom, keys, ctrl_f());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">filter<"), "the head names it: {html}");
        assert_eq!(
            picker_ids(&dom).len(),
            9,
            "one tag, then the eight types: {html}"
        );
        // arrows move the highlight; an unhandled key is absorbed; a
        // second Ctrl+F over the open overlay is inert
        press(&mut dom, filter_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, filter_keys, Key::ArrowUp, Modifiers::empty());
        press(
            &mut dom,
            filter_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        press(&mut dom, keys, ctrl_f(), Modifiers::CONTROL);
        press(&mut dom, filter_keys, ctrl_f(), Modifiers::CONTROL);
        assert_eq!(picker_ids(&dom).len(), 9, "still the one overlay");

        // a query no entry matches: enter guesses nothing, the overlay holds
        type_into(&mut dom, input, "xyzzy");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching filter"), "{html}");
        press(&mut dom, filter_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(">filter<"));

        type_into(&mut dom, input, "method");
        press(&mut dom, filter_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">filter<"), "the overlay closed: {html}");
        assert!(
            html.contains(r#"class="filter-label">tag · method<"#),
            "the chrome names the filter: {html}"
        );
        // beta keeps its ink; everything else dims but stays drawn
        assert!(html.contains(">beta</div>"), "{html}");
        assert!(html.contains(">alpha</div>"), "{html}");
        let dimmed = html.matches("dimmed").count();
        assert_eq!(dimmed, 3, "alpha, the capture and the generated: {html}");
        assert!(
            !html.split(">beta</div>").next().is_some_and(|before| before
                .rsplit("card card-")
                .next()
                .is_some_and(|card| card.contains("dimmed"))),
            "beta itself is not dimmed: {html}"
        );
    }

    #[test]
    fn escape_leaves_the_filter_unchanged_and_empty_enter_clears_it() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            tagged_note("beta", "method"),
        )
        .expect("the tagged note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        // apply the tag filter through a row click
        let mutations =
            press_for_mutations(&mut dom, keys, ctrl_f(), Modifiers::CONTROL);
        let rows = listeners(&mutations, "click");
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        click(&mut dom, rows[0]);
        assert!(dioxus_ssr::render(&dom).contains("filter-label"));

        // escape leaves it standing
        let (_, filter_keys) = open_overlay(&mut dom, keys, ctrl_f());
        press(&mut dom, filter_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("filter-label"), "unchanged: {html}");
        assert!(html.contains("dimmed"), "{html}");

        // the re-summon-and-clear gesture: enter on an empty query
        let (_, filter_keys) = open_overlay(&mut dom, keys, ctrl_f());
        press(&mut dom, filter_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("filter-label"), "cleared: {html}");
        assert!(!html.contains("dimmed"), "{html}");
    }

    #[test]
    fn the_screen_switch_clears_an_open_finder() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_overlay(&mut dom, keys, ctrl_f());
        assert!(dioxus_ssr::render(&dom).contains(">filter<"));

        click(&mut dom, clicks[CHROME_LOGS]);
        click(&mut dom, clicks[CHROME_TABLE]);
        assert!(
            !dioxus_ssr::render(&dom).contains(">filter<"),
            "no overlay waits behind a screen"
        );
    }

    #[test]
    fn ctrl_o_jump_pans_the_card_to_centre_at_the_current_zoom() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (input, jump_keys) = open_overlay(&mut dom, keys, ctrl_o());
        assert!(dioxus_ssr::render(&dom).contains(">jump<"));
        type_into(&mut dom, input, "alpha");
        press(&mut dom, jump_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">jump<"), "the overlay closed: {html}");
        // alpha's slot (32, 32): pan = (640−120, 400−60)
        assert!(
            html.contains("translate(520px, 340px)"),
            "centred at titles zoom: {html}"
        );

        // the same jump at body zoom centres in canvas units
        press(&mut dom, keys, ctrl_equals(), Modifiers::CONTROL);
        let (input, jump_keys) = open_overlay(&mut dom, keys, ctrl_o());
        type_into(&mut dom, input, "alpha");
        press(&mut dom, jump_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("scale(3)"), "no zoom change: {html}");
        assert!(
            html.contains("translate(93.333") && html.contains(", 73.333"),
            "centre/s − card centre: {html}"
        );
    }

    #[test]
    fn jump_offers_only_notes_with_cards() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (input, jump_keys) = open_overlay(&mut dom, keys, ctrl_o());
        assert_eq!(
            picker_ids(&dom),
            vec!["alpha", "capture-idea", "digest"],
            "time notes have no card to jump to"
        );
        // a time id finds nothing, and enter over nothing goes nowhere
        type_into(&mut dom, input, "2026");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching note"), "{html}");
        press(&mut dom, jump_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(">jump<"), "still open");
        // arrows and stray keys are absorbed like every overlay's
        press(&mut dom, jump_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, jump_keys, Key::ArrowUp, Modifiers::empty());
        press(
            &mut dom,
            jump_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        press(&mut dom, keys, ctrl_o(), Modifiers::CONTROL);
        press(&mut dom, jump_keys, ctrl_o(), Modifiers::CONTROL);
        assert!(dioxus_ssr::render(&dom).contains(">jump<"));
    }

    #[test]
    fn clicking_a_jump_row_jumps_too() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        let mutations =
            press_for_mutations(&mut dom, keys, ctrl_o(), Modifiers::CONTROL);
        let rows = listeners(&mutations, "click");
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        // alpha is the first row
        click(&mut dom, rows[0]);
        assert!(dioxus_ssr::render(&dom).contains("translate(520px, 340px)"));
    }

    #[test]
    fn overlays_closed_over_a_closed_editor_skip_the_remount() {
        // an empty selection is the one state without a cursor: escaping
        // an overlay then has no textarea to hand focus back to
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(24)]);

        let (_, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        press(&mut dom, creator_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains("command-palette"));

        // the finders on the table, the editor still closed behind them
        let (_, _, table_keys) = table_targets_with_keys(&mut dom, &clicks);
        let (_, filter_keys) = open_overlay(&mut dom, table_keys, ctrl_f());
        press(&mut dom, filter_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">filter<"));
        let (_, jump_keys) = open_overlay(&mut dom, table_keys, ctrl_o());
        press(&mut dom, jump_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">jump<"));
    }

    #[test]
    fn a_finder_over_a_broken_index_reports_instead_of_opening() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        // the tags table alone vanishes: the open succeeds, the query fails
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE tags")
            .expect("the sabotage succeeds");
        press(&mut dom, keys, ctrl_f(), Modifiers::CONTROL);
        assert!(!dioxus_ssr::render(&dom).contains(">filter<"));

        // the database gone entirely: both finders decline to open
        replace_database_with_a_directory(vault.path());
        press(&mut dom, keys, ctrl_f(), Modifiers::CONTROL);
        press(&mut dom, keys, ctrl_o(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">filter<"), "{html}");
        assert!(!html.contains(">jump<"), "{html}");
    }

    #[test]
    fn a_jump_whose_card_left_meanwhile_pans_nowhere() {
        let vault = temp_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        let (input, jump_keys) = open_overlay(&mut dom, keys, ctrl_o());
        type_into(&mut dom, input, "alpha");

        // the card leaves while the overlay holds its frozen entries
        std::fs::remove_file(vault.path().join("permanent/alpha.typ"))
            .expect("the note is deleted");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Removed(PathBuf::from(
                "permanent/alpha.typ",
            ))],
        );
        press(&mut dom, jump_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">jump<"), "the overlay still closed: {html}");
        assert!(
            html.contains("translate(0px, 0px)"),
            "nowhere to pan to: {html}"
        );
    }

    #[test]
    fn escape_over_a_block_closes_the_finders_and_editing_continues() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, block_keys) = sheet_block_targets(&opened);

        // the filter, opened over the active block, escapes back into it —
        // the widget never unmounted, so the same sink still answers
        let (_, filter_keys) = open_overlay(&mut dom, block_keys, ctrl_f());
        press(&mut dom, filter_keys, Key::Escape, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains("block-active"));

        // and the jump the same way
        let (_, jump_keys) = open_overlay(&mut dom, block_keys, ctrl_o());
        press(&mut dom, jump_keys, Key::Escape, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains("block-active"));
        type_keys(&mut dom, block_keys, "x");
        assert!(
            source_of(&dom).contains('x'),
            "the sink still types: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn the_palette_runs_the_finders() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "filter cards");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">filter<"), "the overlay opened: {html}");

        // the screen round trip clears it; then the jump runs the same way
        click(&mut dom, clicks[CHROME_LOGS]);
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "jump");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(">jump<"));
    }

    // -- semantic zoom: titles ⇄ bodies ---------------------------------------

    /// The zoom chords, spelled once.
    fn ctrl_equals() -> Key {
        Key::Character("=".into())
    }
    fn ctrl_minus() -> Key {
        Key::Character("-".into())
    }

    /// Pans the void so alpha's card (fallback slot 32, 32) sits at the
    /// default viewport's centre — where a zoom keeps it in view.
    fn centre_alpha(dom: &mut VirtualDom, pane: ElementId) {
        mouse(dom, "mousedown", pane, (0.0, 0.0));
        mouse(dom, "mousemove", pane, (520.0, 340.0));
        mouse(dom, "mouseup", pane, (520.0, 340.0));
    }

    #[test]
    fn ctrl_equals_zooms_to_bodies_and_ctrl_minus_back() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        centre_alpha(&mut dom, pane);

        press(&mut dom, keys, ctrl_equals(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("scale(3)"), "{html}");
        assert!(html.contains("card-body"), "bodies render: {html}");
        assert!(html.contains(RENDERED_NOTE), "the note's own svg: {html}");
        assert!(
            !html.contains(">digest</div>"),
            "the card the zoom pushed out is culled: {html}"
        );
        // the same chord again changes nothing — the level already stands
        press(&mut dom, keys, ctrl_equals(), Modifiers::CONTROL);
        assert_eq!(dioxus_ssr::render(&dom), html);

        press(&mut dom, keys, ctrl_minus(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("scale(1)"), "{html}");
        assert!(!html.contains("card-body"), "titles again: {html}");
        // the round trip landed the pan back where it stood
        assert!(
            html.contains("translate(520px, 340px)"),
            "the centre held: {html}"
        );
    }

    #[test]
    fn a_drag_at_body_zoom_moves_in_canvas_units() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        centre_alpha(&mut dom, pane);
        press(&mut dom, keys, ctrl_equals(), Modifiers::CONTROL);

        // 24 client pixels are 8 canvas units at scale 3
        mouse(&mut dom, "mousedown", cards[0], (300.0, 300.0));
        mouse(&mut dom, "mousemove", pane, (324.0, 300.0));
        mouse(&mut dom, "mouseup", pane, (324.0, 300.0));
        assert!(
            dioxus_ssr::render(&dom).contains("left: 40px; top: 32px"),
            "8 canvas units from the slot"
        );
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert_eq!(saved.trim(), "alpha 40 32");
    }

    #[test]
    fn opening_a_sheet_returns_to_titles_zoom() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        centre_alpha(&mut dom, pane);
        press(&mut dom, keys, ctrl_equals(), Modifiers::CONTROL);

        // a click at body zoom zooms out and opens — one legible gesture
        mouse(&mut dom, "mousedown", cards[0], (300.0, 300.0));
        mouse(&mut dom, "mouseup", pane, (300.0, 300.0));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains("scale(1)"), "titles again: {html}");
        assert!(!html.contains("card-body"), "{html}");
    }

    #[test]
    fn an_off_viewport_card_renders_nothing_until_panned_in() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let mutations = click_for_mutations(&mut dom, clicks[CHROME_TABLE]);
        let observer = listeners(&mutations, "resize")[0];
        let pane = listeners(&mutations, "mousedown")[0];
        assert!(
            dioxus_ssr::render(&dom).contains(">digest</div>"),
            "everything shows at the default viewport"
        );

        // the pane shrinks: only the first grid slot stays visible
        resize(&mut dom, observer, 200.0, 200.0);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">alpha</div>"), "{html}");
        assert!(!html.contains(">capture-idea</div>"), "culled: {html}");
        assert!(!html.contains(">digest</div>"), "culled: {html}");

        // an observer that answers nothing keeps the last size
        bare_resize(&mut dom, observer);
        assert_eq!(dioxus_ssr::render(&dom), html);

        // panning brings the neighbour slot into the small viewport
        mouse(&mut dom, "mousedown", pane, (0.0, 0.0));
        mouse(&mut dom, "mousemove", pane, (-192.0, 0.0));
        mouse(&mut dom, "mouseup", pane, (-192.0, 0.0));
        assert!(
            dioxus_ssr::render(&dom).contains(">capture-idea</div>"),
            "panned in"
        );
    }

    #[test]
    fn a_watcher_batch_invalidates_the_body_cache() {
        let vault = temp_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        let (pane, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        centre_alpha(&mut dom, pane);
        press(&mut dom, keys, ctrl_equals(), Modifiers::CONTROL);
        let before = dioxus_ssr::render(&dom);
        assert!(before.contains("card-body"), "{before}");
        assert!(!before.contains("render-error"), "{before}");

        // alpha keeps its meta — and its card — but stops compiling; the
        // batch must drop the cached body, not serve it stale
        std::fs::write(
            vault.path().join("permanent/alpha.typ"),
            format!("{}#let x = (\n", note("alpha")),
        )
        .expect("the note is broken in place");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Permanent,
                path: PathBuf::from("permanent/alpha.typ"),
            }],
        );
        let after = dioxus_ssr::render(&dom);
        assert!(after.contains(">alpha</div>"), "the card held: {after}");
        assert!(
            after.contains("render-error"),
            "the recompiled body reports its error: {after}"
        );
    }

    #[test]
    fn the_zoom_commands_run_through_the_palette_and_hide_in_place() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "zoom");
        assert_eq!(palette_labels(&dom), vec!["zoom to bodies"]);
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains("scale(3)"));

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "zoom");
        assert_eq!(palette_labels(&dom), vec!["zoom to titles"]);
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains("scale(1)"));
    }

    // -- constellations: link edges under the cards --------------------------

    #[test]
    fn the_vaults_links_draw_as_edges_under_the_cards() {
        let vault = temp_vault();
        // beta links alpha; both sit on the fallback grid — (224, 32) and
        // (32, 32) — so the edge runs border to border between the slots
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            linking(note("beta"), "alpha"),
        )
        .expect("the linking note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);

        let drawn = edges_svg(&dom);
        assert!(
            drawn.contains(r#"<line x1="224" y1="60" x2="208" y2="60""#),
            "beta's border to alpha's: {drawn}"
        );
        assert!(
            drawn.contains(r#"<circle cx="224" cy="60" r="2""#),
            "a node dot where the edge meets the card: {drawn}"
        );
        assert!(
            drawn.contains(r#"<circle cx="208" cy="60" r="2""#),
            "and one at the other end: {drawn}"
        );
        // DOM order is the stacking rule: the svg precedes every card
        let html = dioxus_ssr::render(&dom);
        let svg_at = html.find(r#"<svg class="edges""#).expect("the svg");
        let card_at = html.find(r#"class="card "#).expect("a card");
        assert!(svg_at < card_at, "edges paint under the cards: {html}");
    }

    #[test]
    fn dragging_a_card_drags_its_edges() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            linking(note("beta"), "alpha"),
        )
        .expect("the linking note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        // beta is the second card in id order; drag it a card-width right
        mouse(&mut dom, "mousedown", cards[1], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (276.0, 100.0));
        mouse(&mut dom, "mouseup", pane, (276.0, 100.0));
        // the drag pinned beta at (400, 32); alpha, never hand-placed and
        // linked to it, drifted onto beta's ring at (208, −64) — and the
        // edge follows both ends (adr/2026-08-auto-place-strongest-link-ring.md)
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("left: 208px; top: -64px"), "{html}");
        let drawn = edges_svg(&dom);
        assert!(
            drawn.contains(r#"<line x1="432" y1="32" x2="352" y2="-8""#),
            "the edge tracked the drag and the drift: {drawn}"
        );
    }

    #[test]
    fn links_changing_in_the_files_redraw_the_edges_live() {
        let vault = temp_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        assert!(
            !edges_svg(&dom).contains("<line"),
            "the base vault draws no permanent edges"
        );

        // a link written outside the app draws on the watcher batch
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            linking(note("beta"), "alpha"),
        )
        .expect("the linking note is written");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Permanent,
                path: PathBuf::from("permanent/beta.typ"),
            }],
        );
        assert!(edges_svg(&dom).contains("<line"), "the edge arrived");

        // its target deleted, the link dangles — and draws nothing, the
        // debt living in the loops list instead of on the canvas
        std::fs::remove_file(vault.path().join("permanent/alpha.typ"))
            .expect("the target is deleted");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Removed(PathBuf::from(
                "permanent/alpha.typ",
            ))],
        );
        assert!(
            !edges_svg(&dom).contains("<line"),
            "a dangling link draws nothing"
        );
        assert!(
            dioxus_ssr::render(&dom).contains("ember"),
            "the debt went to the loops count instead"
        );
    }

    // -- ctrl+n: creation, and the sheet's delete ----------------------------

    #[test]
    fn ctrl_n_lists_the_eight_types_and_typing_narrows() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">new note<"), "the head names it: {html}");
        assert_eq!(
            picker_ids(&dom),
            vec![
                "person",
                "organisation",
                "source",
                "concept",
                "claim",
                "idea",
                "personal",
                "project"
            ]
        );

        // a second Ctrl+N and a Ctrl+P over the open overlay are inert —
        // whether they land on the pane or bubble from the overlay's own
        // input, which passes ctrl chords through
        press(&mut dom, keys[LOGS_KEYS], ctrl_n(), Modifiers::CONTROL);
        press(&mut dom, keys[LOGS_KEYS], ctrl_p(), Modifiers::CONTROL);
        press(&mut dom, creator_keys, ctrl_n(), Modifiers::CONTROL);
        assert_eq!(picker_ids(&dom).len(), 8, "still the one overlay");
        assert!(!dioxus_ssr::render(&dom).contains("palette-label"));

        // arrows move the highlight and hold at both ends
        let selected = |dom: &VirtualDom| {
            let html = dioxus_ssr::render(dom);
            html.split("picker-row selected")
                .nth(1)
                .and_then(|rest| rest.split(r#"picker-id">"#).nth(1))
                .and_then(|rest| rest.split('<').next())
                .map(str::to_string)
                .unwrap_or_else(|| panic!("no highlighted row: {html}"))
        };
        press(&mut dom, creator_keys, Key::ArrowDown, Modifiers::empty());
        assert_eq!(selected(&dom), "organisation");
        press(&mut dom, creator_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, creator_keys, Key::ArrowUp, Modifiers::empty());
        assert_eq!(selected(&dom), "person", "the first row holds");

        type_into(&mut dom, input, "CON");
        assert_eq!(picker_ids(&dom), vec!["concept"], "narrowed, any case");
        type_into(&mut dom, input, "xyzzy");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching type"), "{html}");
        // enter over no match guesses nothing, and an unhandled key inside
        // the overlay is absorbed, not acted on
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        press(
            &mut dom,
            creator_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert!(dioxus_ssr::render(&dom).contains("no matching type"));
    }

    #[test]
    fn enter_picks_the_type_and_the_input_becomes_the_title() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "concept");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">new concept<"), "the head shows it: {html}");
        assert!(html.contains("title…"), "the input prompts for one: {html}");
        assert!(picker_ids(&dom).is_empty(), "no rows in step 2: {html}");
        // the controlled input emptied with its signal
        assert!(!html.contains(r#"value="concept""#), "{html}");
    }

    #[test]
    fn clicking_a_type_row_picks_it_too() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let mutations = press_for_mutations(
            &mut dom,
            keys[LOGS_KEYS],
            ctrl_n(),
            Modifiers::CONTROL,
        );
        let rows = listeners(&mutations, "click");
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        // the eight types in picker order: concept is the fourth row
        click(&mut dom, rows[3]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">new concept<"), "{html}");
    }

    #[test]
    fn a_titled_enter_writes_the_file_and_opens_the_new_sheet() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "concept");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        type_into(&mut dom, input, "Deep Modules");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());

        let written = vault.path().join("permanent/deep-modules.typ");
        assert!(written.exists(), "the note reached the vault");
        let text = std::fs::read_to_string(&written).expect("the note reads");
        assert!(text.contains("= Deep Modules"), "{text}");
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command-palette"), "overlay closed: {html}");
        assert!(html.contains(r#"class="sheet""#), "sheet opened: {html}");
        assert!(html.contains(RENDERED_NOTE), "on the new note: {html}");
        assert!(html.contains(">Deep Modules</div>"), "its card: {html}");
        assert!(html.contains("bar-concept"), "with its hue: {html}");
        // no viewport injected: the deterministic default centres it —
        // (1280/2 − 88, 800/2 − 28), carried by the raised card at pan 0
        assert!(html.contains("left: 552px; top: 372px"), "{html}");
    }

    #[test]
    fn the_new_card_lands_at_the_injected_viewport_centre() {
        let vault = temp_vault();
        let (mut dom, _, keys) =
            viewport_app(Some(vault.path().to_path_buf()), (800.0, 600.0));
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "concept");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        type_into(&mut dom, input, "Deep Modules");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());

        // 800×600: centre (400, 300) → card corner (400−88, 300−28)
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("left: 312px; top: 272px"), "{html}");
        // a birth slot, not a placement: the positions file holds only
        // hand-drags (adr/2026-08-auto-place-strongest-link-ring.md)
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert!(
            !saved.contains("deep-modules"),
            "creation persists nothing: {saved}"
        );
    }

    #[test]
    fn escape_backs_out_title_to_types_to_closed() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "concept");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(">new concept<"));

        // first escape: back to the type list, whole again
        press(&mut dom, creator_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">new note<"), "step 1 again: {html}");
        assert_eq!(picker_ids(&dom).len(), 8, "the full list is back");

        // second escape: closed
        press(&mut dom, creator_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains("command-palette"));
    }

    #[test]
    fn escape_over_a_block_leaves_the_caret_where_ctrl_n_found_it() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_heading(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 4);

        let (_, creator_keys) = open_creator(&mut dom, keys);
        press(&mut dom, creator_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "{html}");
        // the caret is app state the overlay never touched: still after
        // the fourth character of the heading's first line
        assert!(
            html.contains(r#">= 20</span><span class="caret">"#),
            "{html}"
        );
    }

    #[test]
    fn a_refused_title_keeps_the_overlay_open_with_its_notice() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "concept");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());

        // "Alpha" kebabs to the id the vault already holds
        type_into(&mut dom, input, "Alpha");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("already exists"), "{html}");
        assert!(html.contains("command-palette"), "still open: {html}");
        assert!(vault.path().join("permanent/alpha.typ").exists());

        // amended to nothing usable: the other refusal, same place
        type_into(&mut dom, input, "???");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("at least one letter or digit"), "{html}");

        // typing again clears the notice
        type_into(&mut dom, input, "?!");
        assert!(!dioxus_ssr::render(&dom).contains("at least one letter"));
    }

    #[test]
    fn ctrl_n_answers_on_the_table_too() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_creator(&mut dom, keys);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">new note<"), "{html}");
    }

    #[test]
    fn the_palette_runs_new_note_and_offers_delete_only_over_a_sheet() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // on the logs, no sheet: new note is offered, delete note is not
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        assert!(!palette_labels(&dom).contains(&"delete note".to_string()));
        type_into(&mut dom, input, "new note");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">new note<"), "the creator opened: {html}");
        assert_eq!(picker_ids(&dom).len(), 8, "{html}");
    }

    #[test]
    fn delete_from_the_sheet_removes_file_position_and_card() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);

        // a jittered click: the position writes and the sheet opens
        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (103.0, 98.0));
        mouse(&mut dom, "mouseup", pane, (103.0, 98.0));
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        let (input, palette_keys) = open_palette(&mut dom, keys);
        assert!(palette_labels(&dom).contains(&"delete note".to_string()));
        type_into(&mut dom, input, "delete");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            !vault.path().join("permanent/alpha.typ").exists(),
            "no confirmation, no trash"
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(!html.contains(">alpha</div>"), "the card left: {html}");
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert!(!saved.contains("alpha"), "the position dropped: {saved}");
    }

    #[test]
    fn a_delete_the_filesystem_refuses_keeps_the_sheet_with_the_error() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        // the category directory refuses the unlink
        use std::os::unix::fs::PermissionsExt;
        let dir = vault.path().join("permanent");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))
            .expect("the sabotage takes");

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
            .expect("the sabotage lifts");

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="sheet""#), "still open: {html}");
        assert!(html.contains("delete:"), "the error surfaced: {html}");
        assert!(vault.path().join("permanent/alpha.typ").exists());
    }

    #[test]
    fn deleting_an_error_sheet_just_closes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        replace_database_with_a_directory(vault.path());
        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(dioxus_ssr::render(&dom).contains("sheet:"));

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        // a closed editor holds no file: nothing to delete, and the sheet
        // simply closes
        assert!(!dioxus_ssr::render(&dom).contains(r#"class="sheet""#));
        assert!(vault.path().join("permanent/alpha.typ").exists());
    }

    #[test]
    fn a_delete_whose_sheet_left_meanwhile_does_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        // the palette freezes "a sheet is open" and narrows to the delete…
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete");
        assert_eq!(palette_labels(&dom), vec!["delete note"]);
        // …then the sheet closes under it — the pane's escape, reached
        // directly here, is the race the guard below defends against
        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(r#"class="sheet""#));
        assert!(
            dioxus_ssr::render(&dom).contains("command-palette"),
            "the palette outlived the sheet"
        );
        // running the frozen command now finds no sheet and deletes nothing
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(vault.path().join("permanent/alpha.typ").exists());
        assert!(
            !dioxus_ssr::render(&dom).contains("command-palette"),
            "the run still closed the palette"
        );
    }

    // -- promotion: a capture typed in the editor recolours ------------------

    #[test]
    fn promotion_recolours_through_the_watcher() {
        let vault = temp_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("card card-capture bar-untyped"), "{html}");
        assert_eq!(html.matches("bar-concept").count(), 1, "alpha's: {html}");

        // promotion is editing, nothing more: the type lands in #meta, the
        // summary below (adr/2026-08-typed-capture-wears-its-hue.md); the
        // autosave's disk write flows through this same watcher path
        std::fs::write(
            vault.path().join("capture/capture-idea.typ"),
            format!(
                "#import \"/templates/template.typ\": *\n\
                 #show: note\n\
                 #meta(id: \"capture-idea\", type: \"concept\", \
                 created: \"{TODAY}\")\n\
                 \n= capture-idea\n\
                 \n== Summary\n\nce que ça disait\n"
            ),
        )
        .expect("the promotion is written");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Capture,
                path: PathBuf::from("capture/capture-idea.typ"),
            }],
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("card-capture"), "the dim fill left: {html}");
        assert_eq!(
            html.matches("card card-permanent bar-concept").count(),
            2,
            "the capture wears the hue and full fill now: {html}"
        );
    }

    // -- harness -------------------------------------------------------------

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

    /// Builds the App headlessly with the vault and a fixed clock injected
    /// as root context — the same channels `main` uses — and returns the
    /// click, keydown and wheel targets from the initial mutations. All
    /// three must come from the one rebuild: a second `rebuild_to_vec`
    /// would reassign every ElementId.
    fn rendered_app(
        root: Option<PathBuf>,
    ) -> (VirtualDom, Vec<ElementId>, Vec<ElementId>, Vec<ElementId>) {
        let (dom, mutations) = mounted_app(root, None);
        let clicks = listeners(&mutations, "click");
        let keys = listeners(&mutations, "keydown");
        let wheels = listeners(&mutations, "wheel");
        (dom, clicks, keys, wheels)
    }

    /// Like `rendered_app`, but with a recording `Closer` injected — the
    /// harness for the quit chord — returning the click targets, the
    /// app-root keydown target and the flag the closer sets.
    fn quit_app(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        ElementId,
        Arc<std::sync::atomic::AtomicBool>,
    ) {
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let recorder = closed.clone();
        let closer = Closer(Arc::new(move || {
            recorder.store(true, Ordering::SeqCst);
        }));
        let (dom, mutations) = mounted_app(root, Some(closer));
        let clicks = listeners(&mutations, "click");
        let keydown = listeners(&mutations, "keydown")[0];
        (dom, clicks, keydown, closed)
    }

    /// Mounts the App without a vault — the theme wrapper encloses the error
    /// screen too, so this is the cheapest mount — and returns the keydown
    /// target on the `.app` root.
    fn theme_app() -> (VirtualDom, ElementId) {
        let (dom, mutations) = mounted_app(None, None);
        let keydown = listeners(&mutations, "keydown")[0];
        (dom, keydown)
    }

    fn mounted_app(
        root: Option<PathBuf>,
        closer: Option<Closer>,
    ) -> (VirtualDom, Mutations) {
        mounted_app_with_hit(root, closer, None)
    }

    fn mounted_app_with_hit(
        root: Option<PathBuf>,
        closer: Option<Closer>,
        hit: Option<HitProbe>,
    ) -> (VirtualDom, Mutations) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        if let Some(closer) = closer {
            dom.insert_any_root_context(Box::new(closer));
        }
        if let Some(hit) = hit {
            dom.insert_any_root_context(Box::new(hit));
        }
        let mutations = dom.rebuild_to_vec();
        (dom, mutations)
    }

    /// A hit cell holding this never answers.
    const HIT_HANGS: (usize, usize) = (usize::MAX, usize::MAX);

    /// Like `rendered_app`, but with a scripted hit probe injected: each
    /// mouse press reads whatever the returned cell holds at that moment —
    /// (span `data-start`, UTF-16 units within it), or `None` for a miss.
    #[allow(clippy::type_complexity)]
    fn hit_app(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        Arc<std::sync::Mutex<Option<(usize, usize)>>>,
    ) {
        let landing = Arc::new(std::sync::Mutex::new(None::<(usize, usize)>));
        let feed = landing.clone();
        let hit = HitProbe(Arc::new(move |_, _| {
            let landed = *feed.lock().expect("the hit cell never poisons");
            if landed == Some(HIT_HANGS) {
                // the probe that stays out, for the one-in-flight guard
                return Box::pin(std::future::pending());
            }
            Box::pin(async move { landed })
        }));
        let (dom, mutations) = mounted_app_with_hit(root, None, Some(hit));
        let clicks = listeners(&mutations, "click");
        (dom, clicks, landing)
    }

    /// Puts the caret at `units` (UTF-16, block-relative) through the mouse
    /// path: script the hit probe, press, let the spawned probe land.
    fn place_caret(
        dom: &mut VirtualDom,
        block: ElementId,
        hit: &Arc<std::sync::Mutex<Option<(usize, usize)>>>,
        units: usize,
    ) {
        *hit.lock().expect("the hit cell never poisons") = Some((0, units));
        mouse(dom, "mousedown", block, (0.0, 0.0));
        mouse(dom, "mouseup", block, (0.0, 0.0));
        block_on(settle(dom));
    }

    /// Types text as the sink receives it: one keydown per cluster, Enter
    /// for each newline — the shape real typing has
    /// (adr/2026-08-hidden-ime-sink.md).
    fn type_keys(dom: &mut VirtualDom, sink: ElementId, text: &str) {
        for cluster in
            unicode_segmentation::UnicodeSegmentation::graphemes(text, true)
        {
            if cluster == "\n" {
                press(dom, sink, Key::Enter, Modifiers::empty());
            } else {
                press(
                    dom,
                    sink,
                    Key::Character(cluster.to_string()),
                    Modifiers::empty(),
                );
            }
        }
    }

    /// Replaces the active block's whole content: select all, then type —
    /// the keystroke-honest successor to feeding the textarea a new value.
    fn retype(dom: &mut VirtualDom, sink: ElementId, text: &str) {
        press(dom, sink, Key::Character("a".into()), Modifiers::CONTROL);
        type_keys(dom, sink, text);
    }

    /// Fires one composition event at the sink.
    fn compose(
        dom: &mut VirtualDom,
        sink: ElementId,
        kind: &'static str,
        data: &str,
    ) {
        with_reactor(|| {
            let payload: Rc<dyn Any> = Rc::new(PlatformEventData::new(
                Box::new(FakeComposition(data.to_string())),
            ));
            dom.runtime()
                .handle_event(kind, Event::new(payload, true), sink);
            dom.process_events();
            dom.render_immediate_to_vec();
        });
    }

    /// What the IME hands the sink — the `FakeMount` idiom for composition.
    #[derive(Clone)]
    struct FakeComposition(String);

    impl HasCompositionData for FakeComposition {
        fn data(&self) -> String {
            self.0.clone()
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// The app with a scripted clipboard and a fixed capture clock, for the
    /// in-app capture chord. `now: None` leaves the clock uninjected, the
    /// way a headless run without it would find things.
    fn capture_app(
        root: Option<PathBuf>,
        pasted: Option<String>,
        now: Option<&str>,
    ) -> (VirtualDom, Vec<ElementId>, Vec<ElementId>) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(Clipboard(Arc::new(
            move || {
                let pasted = pasted.clone();
                Box::pin(async move { pasted })
            },
        ))));
        if let Some(now) = now {
            let stamp: jiff::Zoned =
                now.parse().expect("the capture clock is a valid timestamp");
            dom.insert_any_root_context(Box::new(Now(Arc::new(move || {
                stamp.clone()
            }))));
        }
        let mutations = dom.rebuild_to_vec();
        (
            dom,
            listeners(&mutations, "click"),
            listeners(&mutations, "keydown"),
        )
    }

    /// Fires a keydown. The physical code is irrelevant to every handler,
    /// which read only the key and its modifiers.
    fn press(
        dom: &mut VirtualDom,
        target: ElementId,
        key: Key,
        modifiers: Modifiers,
    ) {
        press_for_mutations(dom, target, key, modifiers);
    }

    /// A keydown flagged `isComposing` — the commit keystroke the IME owns,
    /// which the sink must leave alone (adr/2026-08-hidden-ime-sink.md).
    fn press_composing(dom: &mut VirtualDom, target: ElementId, key: Key) {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedKeyboardData::new(
                    key,
                    Code::KeyT,
                    Location::Standard,
                    false,
                    Modifiers::empty(),
                    true,
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

    /// The app with both clipboard seams scripted: reads answer `pasted`,
    /// writes land in the returned recorder — the widget's Ctrl+C/X/V
    /// harness (adr/2026-08-hidden-ime-sink.md).
    #[allow(clippy::type_complexity)]
    fn clipboard_app(
        root: Option<PathBuf>,
        pasted: Option<String>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(Clipboard(Arc::new(
            move || {
                let pasted = pasted.clone();
                Box::pin(async move { pasted })
            },
        ))));
        let written = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = written.clone();
        dom.insert_any_root_context(Box::new(ClipboardWrite(Arc::new(
            move |text| {
                recorder
                    .lock()
                    .expect("the write cell never poisons")
                    .push(text);
                Box::pin(async {})
            },
        ))));
        let mutations = dom.rebuild_to_vec();
        let clicks = listeners(&mutations, "click");
        (dom, clicks, written)
    }

    /// Like `press`, but hands back the mutations it caused — how the
    /// listeners a keystroke mounts (the link picker's input) are harvested.
    fn press_for_mutations(
        dom: &mut VirtualDom,
        target: ElementId,
        key: Key,
        modifiers: Modifiers,
    ) -> Mutations {
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
            dom.render_immediate_to_vec()
        })
    }

    /// Drives the autosave through its restart, its `QUIET` sleep and the
    /// write that follows. Bounded: eight short waits, never a spin.
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
    /// dom tasks (the autosave's sleep) without awaiting anything
    /// themselves.
    fn with_reactor<T>(work: impl FnOnce() -> T) -> T {
        REACTOR.with(|reactor| {
            let _guard = reactor.enter();
            work()
        })
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        REACTOR.with(|reactor| reactor.block_on(future))
    }

    fn click(dom: &mut VirtualDom, target: ElementId) {
        click_for_mutations(dom, target);
    }

    /// A mousedown with Ctrl held — the press that follows a link in the
    /// source (adr/2026-08-ctrl-enter-opens-time-links.md).
    fn ctrl_press(dom: &mut VirtualDom, target: ElementId) {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedMouseData::new(
                    Some(input_data::MouseButton::Primary),
                    input_data::MouseButton::Primary.into(),
                    {
                        use dioxus::html::geometry::*;
                        Coordinates::new(
                            ScreenPoint::zero(),
                            ClientPoint::zero(),
                            ElementPoint::zero(),
                            PagePoint::zero(),
                        )
                    },
                    Modifiers::CONTROL,
                ),
            )));
            dom.runtime().handle_event(
                "mousedown",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            let _ = dom.render_immediate_to_vec();
        })
    }

    /// Fires a resize with the given observed size — how the tests stand in
    /// for the pane's ResizeObserver
    /// (adr/2026-08-viewport-culling-onresize.md).
    fn resize(
        dom: &mut VirtualDom,
        target: ElementId,
        width: f64,
        height: f64,
    ) {
        use dioxus::html::geometry::PixelsSize;
        let size = PixelsSize::new(width, height);
        deliver_resize(
            dom,
            target,
            Rc::new(PlatformEventData::new(Box::new(
                SerializedResizeData::new(size, size),
            ))),
        );
    }

    /// A resize whose observer refuses to answer: the handler must keep the
    /// viewport it has.
    fn bare_resize(dom: &mut VirtualDom, target: ElementId) {
        deliver_resize(
            dom,
            target,
            Rc::new(PlatformEventData::new(Box::new(BareResize))),
        );
    }

    fn deliver_resize(
        dom: &mut VirtualDom,
        target: ElementId,
        data: Rc<dyn Any>,
    ) {
        with_reactor(|| {
            dom.runtime().handle_event(
                "resize",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            dom.render_immediate_to_vec();
        });
    }

    /// Fires one mouse event of the given kind at the target, carrying real
    /// client coordinates — the space the table's pan and drag math reads.
    fn mouse(
        dom: &mut VirtualDom,
        kind: &'static str,
        target: ElementId,
        at: (f64, f64),
    ) {
        mouse_for_mutations(dom, kind, target, at);
    }

    /// Like `mouse`, but hands back the mutations it caused — how the
    /// listeners the sheet mounts on its opening mouseup are harvested.
    fn mouse_for_mutations(
        dom: &mut VirtualDom,
        kind: &'static str,
        target: ElementId,
        at: (f64, f64),
    ) -> Mutations {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedMouseData::new(
                    Some(input_data::MouseButton::Primary),
                    input_data::MouseButton::Primary.into(),
                    {
                        use dioxus::html::geometry::*;
                        Coordinates::new(
                            ScreenPoint::zero(),
                            ClientPoint::new(at.0, at.1),
                            ElementPoint::zero(),
                            PagePoint::zero(),
                        )
                    },
                    Modifiers::empty(),
                ),
            )));
            dom.runtime()
                .handle_event(kind, Event::new(data, true), target);
            dom.process_events();
            dom.render_immediate_to_vec()
        })
    }

    /// Switches to the table and hands back its mousedown targets: the pane
    /// itself (the void — also the mousemove/mouseup target), then each
    /// card's, in card order. Established empirically like the click
    /// constants: the pane registers ahead of its cards.
    fn table_targets(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, Vec<ElementId>) {
        let (pane, cards, _) = table_targets_with_keys(dom, clicks);
        (pane, cards)
    }

    /// Like `table_targets`, but also hands back the table pane's keydown
    /// target — where the sheet's escape and the editor chords land.
    fn table_targets_with_keys(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, Vec<ElementId>, ElementId) {
        let mutations = click_for_mutations(dom, clicks[CHROME_TABLE]);
        let downs = listeners(&mutations, "mousedown");
        let keys = listeners(&mutations, "keydown")[0];
        (downs[0], downs[1..].to_vec(), keys)
    }

    /// Clicks a card open: a press and release on the same point, handing
    /// back the mutations of the mouseup that mounted the sheet — its
    /// fragment clicks, and the raised card's and aside's mousedowns.
    fn open_sheet_on(
        dom: &mut VirtualDom,
        pane: ElementId,
        card: ElementId,
    ) -> Mutations {
        mouse(dom, "mousedown", card, (100.0, 100.0));
        mouse_for_mutations(dom, "mouseup", pane, (100.0, 100.0))
    }

    /// Like `click`, but hands back the mutations it caused — how the
    /// listeners a click mounts (the active textarea's input and keydown)
    /// are harvested, since they are never in the initial table.
    fn click_for_mutations(
        dom: &mut VirtualDom,
        target: ElementId,
    ) -> Mutations {
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
            dom.render_immediate_to_vec()
        })
    }

    /// Fires an input event carrying the textarea's whole new value — the
    /// shape the oninput handler reads through `event.value()` — without
    /// driving the debounced autosave, leaving the buffer dirty on purpose.
    fn type_into(dom: &mut VirtualDom, target: ElementId, text: &str) {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedFormData::new(text.to_string(), Vec::new()),
            )));
            dom.runtime().handle_event(
                "input",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            dom.render_immediate_to_vec();
        });
    }

    /// The picker's rows, in order — the assertions want the list itself,
    /// not a substring of a page that also holds the rail.
    fn picker_ids(dom: &VirtualDom) -> Vec<String> {
        dioxus_ssr::render(dom)
            .split(r#"<span class="picker-id">"#)
            .skip(1)
            .filter_map(|rest| rest.split('<').next().map(str::to_string))
            .collect()
    }

    /// The active block's drawn text, reassembled from its source lines and
    /// unescaped — the widget renders the source as spans, so the page is
    /// read the way a reader would: line by line, tags stripped. The
    /// zero-width caret span contributes nothing.
    fn source_of(dom: &VirtualDom) -> String {
        let html = dioxus_ssr::render(dom);
        let lines: Vec<String> = html
            .split(r#"<div class="source-line">"#)
            .skip(1)
            .filter_map(|rest| rest.split("</div>").next())
            .map(|line| {
                // the composition preview is drawn but not buffer content
                let line: String = line
                    .split(r#"<span class="compose""#)
                    .enumerate()
                    .map(|(index, part)| {
                        if index == 0 {
                            part.to_string()
                        } else {
                            part.split_once("</span>")
                                .map(|(_, rest)| rest)
                                .unwrap_or("")
                                .to_string()
                        }
                    })
                    .collect();
                line.split('<')
                    .map(|chunk| {
                        chunk
                            .split_once('>')
                            .map(|(_, text)| text.to_string())
                            .unwrap_or_else(|| chunk.to_string())
                    })
                    .collect::<String>()
            })
            .collect();
        lines
            .join("\n")
            .replace("&#34;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }

    /// The link chord, spelled once.
    fn ctrl_l() -> Key {
        Key::Character("l".into())
    }

    /// The palette chord, spelled once.
    fn ctrl_p() -> Key {
        Key::Character("p".into())
    }

    /// The create chord, spelled once.
    fn ctrl_n() -> Key {
        Key::Character("n".into())
    }

    /// The finder chords, spelled once.
    fn ctrl_f() -> Key {
        Key::Character("f".into())
    }
    fn ctrl_o() -> Key {
        Key::Character("o".into())
    }

    /// Opens whichever overlay the chord summons and returns its input's
    /// (input, keydown) targets — `open_palette`, generalized.
    fn open_overlay(
        dom: &mut VirtualDom,
        keys: ElementId,
        chord: Key,
    ) -> (ElementId, ElementId) {
        let mutations =
            press_for_mutations(dom, keys, chord, Modifiers::CONTROL);
        let inputs = listeners(&mutations, "input");
        let keydowns = listeners(&mutations, "keydown");
        mount(dom, listeners(&mutations, "mounted")[0]);
        (inputs[0], keydowns[0])
    }

    /// Opens the create overlay with Ctrl+N and returns its input's (input,
    /// keydown) targets — `open_palette` for the third overlay. The input
    /// is controlled and survives the step transition, so these targets
    /// serve both steps.
    fn open_creator(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, ElementId) {
        let mutations =
            press_for_mutations(dom, keys, ctrl_n(), Modifiers::CONTROL);
        let inputs = listeners(&mutations, "input");
        let keydowns = listeners(&mutations, "keydown");
        mount(dom, listeners(&mutations, "mounted")[0]);
        (inputs[0], keydowns[0])
    }

    /// `rendered_app` with a fixed viewport injected — the harness for the
    /// tests that prove the injection is read
    /// (adr/2026-08-new-card-lands-at-viewport-centre.md).
    fn viewport_app(
        root: Option<PathBuf>,
        size: (f64, f64),
    ) -> (VirtualDom, Vec<ElementId>, Vec<ElementId>) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(Viewport(Arc::new(move || {
            size
        }))));
        let mutations = dom.rebuild_to_vec();
        (
            dom,
            listeners(&mutations, "click"),
            listeners(&mutations, "keydown"),
        )
    }

    /// Opens the palette with Ctrl+P and returns its input's (input,
    /// keydown) targets — `open_picker`, one overlay over.
    fn open_palette(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, ElementId) {
        let mutations =
            press_for_mutations(dom, keys, ctrl_p(), Modifiers::CONTROL);
        let inputs = listeners(&mutations, "input");
        let keydowns = listeners(&mutations, "keydown");
        mount(dom, listeners(&mutations, "mounted")[0]);
        (inputs[0], keydowns[0])
    }

    /// The constellation svg's inner markup — the chrome icons also hold
    /// `<line>` elements, so edge assertions must stay inside the one svg.
    fn edges_svg(dom: &VirtualDom) -> String {
        dioxus_ssr::render(dom)
            .split(r#"<svg class="edges""#)
            .nth(1)
            .and_then(|rest| rest.split("</svg>").next())
            .unwrap_or_default()
            .to_string()
    }

    /// The palette's rows, in order — `picker_ids` for the other overlay.
    fn palette_labels(dom: &VirtualDom) -> Vec<String> {
        dioxus_ssr::render(dom)
            .split(r#"<span class="palette-label">"#)
            .skip(1)
            .filter_map(|rest| rest.split('<').next().map(str::to_string))
            .collect()
    }

    /// Opens the picker with Ctrl+L and returns its input's (input, keydown)
    /// targets. The probe cell must already hold the anchor. The mount
    /// event is delivered too — the query field asks for focus the way the
    /// active textarea does, and a headless refusal is what it must absorb.
    fn open_picker(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, ElementId) {
        let mutations =
            press_for_mutations(dom, keys, ctrl_l(), Modifiers::CONTROL);
        let inputs = listeners(&mutations, "input");
        let keydowns = listeners(&mutations, "keydown");
        mount(dom, listeners(&mutations, "mounted")[0]);
        (inputs[0], keydowns[0])
    }

    /// The logs pane's own targets, for the tests that press a key with no
    /// block active.
    fn pane_targets(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, ElementId, ElementId) {
        // a click on a rail row re-renders without mounting a textarea, so
        // the pane keydown target is still the one from the initial mount;
        // the fallback must be an element *inside* the pane — a keydown
        // only reaches the pane handler by bubbling, and the chrome icons
        // at the front of `clicks` sit outside it
        let mutations = click_for_mutations(dom, clicks[RAIL_DAY_23]);
        let keys = listeners(&mutations, "keydown");
        let target = *keys.last().unwrap_or(&clicks[CAL_BACK]);
        (clicks[CAL_BACK], target, target)
    }

    /// Makes `Index::open` fail for every later read: the database path
    /// becomes a directory, which SQLite cannot open.
    fn replace_database_with_a_directory(vault: &Path) {
        let db = vault.join(".index/index.db");
        std::fs::remove_file(&db).expect("the database is removed");
        std::fs::create_dir(&db).expect("a directory takes its place");
    }

    /// Activates a block and returns the textarea's (input, keydown)
    /// targets from the mount mutations.
    /// Wakes a rendered block and hands back the widget's two targets: the
    /// block div's mousedown (where presses land) and the sink's keydown
    /// (where typing lands) — the woken widget's own listeners are in the
    /// click's mutations.
    fn activate_block(
        dom: &mut VirtualDom,
        block: ElementId,
    ) -> (ElementId, ElementId) {
        let mutations = click_for_mutations(dom, block);
        let downs = listeners(&mutations, "mousedown");
        let keys = listeners(&mutations, "keydown");
        (downs[0], keys[0])
    }

    /// The heading widget's targets. A note opens with its last block —
    /// the fixture's heading — already active, so the old
    /// click-to-activate is a bounce: activate the preamble, then click
    /// the heading fragment back into a fresh widget, landing on the
    /// exact state the pre-cursor tests started from.
    fn activate_heading(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, ElementId) {
        let bounced = click_for_mutations(dom, clicks[BLOCK_PREAMBLE]);
        // the woken preamble widget carries no click listener, so the
        // heading fragment's is the bounce's only one
        let heading = listeners(&bounced, "click")[0];
        activate_block(dom, heading)
    }

    /// The sheet's active widget targets: the sheet opens with the note's
    /// last block already awake, its listeners in the opening mutations —
    /// the raised card's and aside's mousedowns register ahead of the
    /// block's (adr/2026-08-cursor-always-in-the-note.md).
    fn sheet_block_targets(opened: &Mutations) -> (ElementId, ElementId) {
        (
            listeners(opened, "mousedown")[2],
            listeners(opened, "keydown")[0],
        )
    }

    /// Fires a wheel event with the given vertical pixel delta.
    fn scroll(dom: &mut VirtualDom, target: ElementId, delta_y: f64) {
        with_reactor(|| {
            let data: Rc<dyn Any> = Rc::new(PlatformEventData::new(Box::new(
                SerializedWheelData {
                    mouse: SerializedPointInteraction::default(),
                    delta_mode: 0, // pixels
                    delta_x: 0.0,
                    delta_y,
                    delta_z: 0.0,
                },
            )));
            dom.runtime().handle_event(
                "wheel",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            dom.render_immediate_to_vec();
        });
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

    /// A vault living in the fixture week: three daily notes (one that
    /// cannot compile), the week, the season, one permanent note that must
    /// never surface, and a capture + generated pair created on `TODAY`
    /// for the "captured today" block.
    /// `temp_vault` plus one of each kind of open loop: a note that never
    /// picked a type, a link to nothing, and a capture that never got its
    /// summary. The base vault is deliberately loop-free, so every ember
    /// test starts here instead.
    fn debt_vault() -> tempfile::TempDir {
        let dir = temp_vault();
        for (path, text) in [
            (
                "permanent/mystere.typ",
                "#import \"/templates/template.typ\": *\n\
                 #show: note\n\
                 #meta(id: \"mystere\", created: \"2026-07-01\")\n\
                 \n= mystere\n"
                    .to_string(),
            ),
            (
                "permanent/linky.typ",
                format!("{}#l(\"ghost\")\n", note("linky")),
            ),
            (
                "capture/capture-zettel.typ",
                format!(
                    "#import \"/templates/template.typ\": *\n\
                     #show: note\n\
                     #meta(id: \"capture-zettel\", created: \"{TODAY}\")\n\
                     \n== Summary\n\n== Original\n\ncollé du navigateur\n"
                ),
            ),
        ] {
            std::fs::write(dir.path().join(path), text)
                .expect("the debt note is written");
        }
        dir
    }

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
            ("templates/daily.typ", time_template("daily")),
            ("templates/weekly.typ", time_template("weekly")),
            ("templates/seasonal.typ", time_template("seasonal")),
            ("templates/concept.typ", permanent_template("concept")),
            ("permanent/alpha.typ", note("alpha")),
            (
                "time/2026-07-21.typ",
                format!("{}#let x = (\n", time_note("2026-07-21", "daily")),
            ),
            // the two link directions the footer shows, both resolving so
            // the vault still opens with zero loops
            (
                "time/2026-07-22.typ",
                linking(time_note("2026-07-22", "daily"), "2026-07-23"),
            ),
            (
                "time/2026-07-23.typ",
                linking(time_note("2026-07-23", "daily"), "2026-07-22"),
            ),
            ("time/2026-w30.typ", time_note("2026-w30", "weekly")),
            ("time/2026-summer.typ", time_note("2026-summer", "seasonal")),
            // the shape the real one has: an empty Summary over the paste,
            // which is what makes a fresh capture an open loop
            (
                "templates/capture.typ",
                "#import \"/templates/template.typ\": *\n\
                 #show: note\n\
                 #meta(id: \"{{id}}\", created: \"{{created}}\")\n\
                 \n== Summary\n\n== Original\n\n{{content}}\n"
                    .to_string(),
            ),
            (
                // summarized, so the base vault still opens with zero open
                // loops and the ember stays absent — every listener index
                // below is counted without it
                "capture/capture-idea.typ",
                format!(
                    "#import \"/templates/template.typ\": *\n\
                     #show: note\n\
                     #meta(id: \"capture-idea\", created: \"{TODAY}\")\n\
                     \n= capture-idea\n\
                     \n== Summary\n\nce que ça disait\n"
                ),
            ),
            (
                "generated/digest.typ",
                format!(
                    "#import \"/templates/template.typ\": *\n\
                     #show: note\n\
                     #meta(id: \"digest\", type: \"generated\", \
                     created: \"{TODAY}\")\n\
                     \n= digest\n"
                ),
            ),
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

    fn time_template(type_name: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{{{{id}}}}\", type: \"{type_name}\", \
             created: \"{{{{created}}}}\")\n\
             \n= {{{{id}}}}\n"
        )
    }

    /// A permanent type's template, mirroring the real fixtures: the title,
    /// not the id, heads the note.
    fn permanent_template(type_name: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{{{{id}}}}\", type: \"{type_name}\", \
             created: \"{{{{created}}}}\")\n\
             \n= {{{{title}}}}\n"
        )
    }

    fn time_note(id: &str, type_name: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{id}\", type: \"{type_name}\", \
             created: \"2026-07-01\")\n\
             \n= {id}\n"
        )
    }

    /// Adds a link to the note's heading block — no blank line, so the block
    /// count the editor tests count on is unchanged.
    fn linking(note: String, target: &str) -> String {
        format!("{note}#l(\"{target}\")\n")
    }

    fn note(id: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{id}\", type: \"concept\", created: \"2026-07-01\")\n\
             \n= {id}\n"
        )
    }

    /// A permanent note carrying one tag, for the filter tests.
    fn tagged_note(id: &str, tag: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{id}\", type: \"idea\", created: \"2026-07-01\", \
             tags: (\"{tag}\",))\n\
             \n= {id}\n"
        )
    }

    /// A resize whose observer answers nothing: every backing method keeps
    /// its NotSupported default — the refusal branch the onresize handler
    /// absorbs by keeping the current viewport.
    struct BareResize;

    impl HasResizeData for BareResize {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// What the textarea's onmounted receives in the headless tests: every
    /// backing method keeps its NotSupported default, which is exactly what
    /// the focus request has to shrug off.
    struct FakeMount;

    impl RenderedElementBacking for FakeMount {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// A backing that answers focus requests instead of refusing them, and
    /// counts them — how the pane proves it took focus back.
    struct FocusMount(Arc<std::sync::atomic::AtomicUsize>);

    impl RenderedElementBacking for FocusMount {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn set_focus(
            &self,
            _focus: bool,
        ) -> Pin<Box<dyn Future<Output = dioxus::html::MountedResult<()>>>>
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }
    }

    /// Fires the mounted event the renderer would deliver for the freshly
    /// swapped-in textarea.
    fn mount(dom: &mut VirtualDom, target: ElementId) {
        deliver_mount(dom, target, FakeMount);
    }

    /// Like `mount`, but the element answers focus requests and counts them.
    fn mount_counting_focus(
        dom: &mut VirtualDom,
        target: ElementId,
    ) -> Arc<std::sync::atomic::AtomicUsize> {
        let focused = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        deliver_mount(dom, target, FocusMount(focused.clone()));
        focused
    }

    fn deliver_mount<B: RenderedElementBacking + 'static>(
        dom: &mut VirtualDom,
        target: ElementId,
        backing: B,
    ) {
        with_reactor(|| {
            let data: Rc<dyn Any> =
                Rc::new(PlatformEventData::new(Box::new(backing)));
            // mounted announces one element and never bubbles in the real
            // renderer; a bubbled one here would also reach the pane's
            // onmounted and swap its recorded handle for this backing
            dom.runtime().handle_event(
                "mounted",
                Event::new(data, false),
                target,
            );
            dom.process_events();
            dom.render_immediate_to_vec();
        });
    }

    /// Only mouse, keyboard, wheel, form and mounted events are real: the
    /// shell listens for clicks everywhere, keydowns on the two roots and
    /// the textarea, wheel on the jump panel, input and mounted on the
    /// textarea — every other conversion is unreachable in these tests.
    struct TestEvents;

    impl HtmlEventConverter for TestEvents {
        fn convert_mouse_data(&self, event: &PlatformEventData) -> MouseData {
            event
                .downcast::<SerializedMouseData>()
                .cloned()
                .map(MouseData::from)
                .expect("the tests only fire serialized mouse events")
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

        fn convert_wheel_data(&self, event: &PlatformEventData) -> WheelData {
            event
                .downcast::<SerializedWheelData>()
                .cloned()
                .map(WheelData::from)
                .expect("the tests only fire serialized wheel events")
        }

        fn convert_animation_data(
            &self,
            _: &PlatformEventData,
        ) -> AnimationData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_cancel_data(&self, _: &PlatformEventData) -> CancelData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_clipboard_data(
            &self,
            _: &PlatformEventData,
        ) -> ClipboardData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_composition_data(
            &self,
            event: &PlatformEventData,
        ) -> CompositionData {
            event
                .downcast::<FakeComposition>()
                .cloned()
                .map(CompositionData::from)
                .expect("the tests only fire fake composition events")
        }

        fn convert_drag_data(&self, _: &PlatformEventData) -> DragData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_focus_data(&self, _: &PlatformEventData) -> FocusData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_form_data(&self, event: &PlatformEventData) -> FormData {
            event
                .downcast::<SerializedFormData>()
                .cloned()
                .map(FormData::from)
                .expect("the tests only fire serialized form events")
        }

        fn convert_image_data(&self, _: &PlatformEventData) -> ImageData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_media_data(&self, _: &PlatformEventData) -> MediaData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_mounted_data(
            &self,
            event: &PlatformEventData,
        ) -> MountedData {
            // two backings: one that refuses focus the way a headless
            // element does, one that grants and counts it
            match event.downcast::<FocusMount>() {
                Some(counter) => {
                    MountedData::from(FocusMount(counter.0.clone()))
                }
                None => event
                    .downcast::<FakeMount>()
                    .map(|_| MountedData::from(FakeMount))
                    .expect("the tests only fire fake mount events"),
            }
        }

        fn convert_pointer_data(&self, _: &PlatformEventData) -> PointerData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_resize_data(
            &self,
            event: &PlatformEventData,
        ) -> ResizeData {
            // two backings, the mounted converter's idiom: the serialized
            // size, or a bare observer whose refusal the handler absorbs
            match event.downcast::<SerializedResizeData>() {
                Some(data) => ResizeData::from(data.clone()),
                None => ResizeData::new(BareResize),
            }
        }

        fn convert_scroll_data(&self, _: &PlatformEventData) -> ScrollData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_selection_data(
            &self,
            _: &PlatformEventData,
        ) -> SelectionData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_toggle_data(&self, _: &PlatformEventData) -> ToggleData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_touch_data(&self, _: &PlatformEventData) -> TouchData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_transition_data(
            &self,
            _: &PlatformEventData,
        ) -> TransitionData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_visible_data(&self, _: &PlatformEventData) -> VisibleData {
            unreachable!("the shell never listens for this event")
        }
    }
}
