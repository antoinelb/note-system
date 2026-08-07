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
use crate::domain::{NoteCategory, NoteType};
use crate::editor::Editor;
use crate::index::{Index, IndexError};
use crate::links;
use crate::logs::{self, Selection};
use crate::loops;
use crate::palette;
use crate::render::{FragmentCache, RenderTheme};
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

/// How a boundary arrow reads the active textarea's caret: `main` injects a
/// JS `selectionStart` probe (UTF-16 code units; None when nothing
/// applies), the headless tests inject scripted fakes — the `Closer`
/// pattern (adr/2026-07-hybrid-active-block-textarea.md).
#[derive(Clone)]
pub struct CaretProbe(
    #[allow(clippy::type_complexity)]
    pub  Arc<
        dyn Fn() -> Pin<Box<dyn Future<Output = Option<usize>>>> + Send + Sync,
    >,
);

/// How an accepted completion puts the caret back after the link it wrote:
/// `main` injects a JS `setSelectionRange`, the headless tests inject a
/// recorder — the `CaretProbe` pattern in the other direction
/// (adr/2026-08-ctrl-l-link-picker.md).
#[derive(Clone)]
pub struct CaretWriter(
    #[allow(clippy::type_complexity)]
    pub  Arc<dyn Fn(usize) -> Pin<Box<dyn Future<Output = ()>>> + Send + Sync>,
);

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
                    Ok((root, notes, loops)) => {
                        rsx! { Shell { root, notes, loops, today: today.0 } }
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
    let mut loops_open = use_signal(|| false);
    let mut selected = use_signal(|| (NoteType::Daily, time::day_id(today)));
    let mut month = use_signal(|| today.first_of_month());
    // the fragment cache is a memo store, not UI state: nothing should
    // re-render when it fills, so a plain hook value rather than a signal
    let fragments =
        use_hook(|| Rc::new(RefCell::new(FragmentCache::default())));
    // absent in headless tests that don't inject a fake: arrows then stay
    // ordinary caret movement
    let probe = try_consume_context::<CaretProbe>();
    let writer = try_consume_context::<CaretWriter>();
    let clipboard = try_consume_context::<Clipboard>();
    let now = try_consume_context::<Now>();
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
    // the uncontrolled textarea only shows a spliced-in link if it remounts,
    // and it remounts when its key changes — keystrokes never touch this
    let mut epoch = use_signal(|| 0u32);
    // where the caret goes once that remount lands; a plain cell, like
    // QuitFlush, because only the mount handler ever reads it
    let pending_caret = use_hook(|| Rc::new(std::cell::Cell::new(None)));

    // the Ctrl+Q flush: reports whether the open note reached disk, so a
    // failed save can hold the app open instead of losing the buffer
    let quit_flush = use_callback(move |()| editor.write().flush());
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
                    match refresh(&root, &batch) {
                        Ok((time_notes, open)) => {
                            notes.set(time_notes);
                            loops.set(open);
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

    // the small movements, lifted so chord, button, wheel and palette all
    // run one path (adr/2026-08-palette-birth-command-list.md)
    let page = use_callback(move |forward: bool| {
        month.set(logs::page_month(month(), forward));
    });
    let toggle_loops = use_callback(move |()| loops_open.set(!loops_open()));
    let go_today = use_callback(move |()| {
        select.call((NoteType::Daily, time::day_id(today)))
    });

    // Where the logs pane is, so focus can be put back on it. A keydown
    // only bubbles up from whatever has focus, and the window's chords
    // (Ctrl+Q, Ctrl+T) are handled on the app root — so when the active
    // block's textarea unmounts, the webview drops focus on `<body>`, which
    // is *above* the app and outside every handler it has, and the chords
    // go dead until something inside is clicked. A plain cell, like
    // QuitFlush: nothing re-renders when the pane announces itself.
    let pane = use_hook(|| Rc::new(RefCell::new(None::<Rc<MountedData>>)));
    use_effect({
        let pane = pane.clone();
        move || {
            // all three are read every time, so the effect follows them
            // all. While the palette is up the pane must not take focus —
            // the palette's input just asked for it in its own mount; when
            // it closes, this re-runs and the pane gets it back
            let editing = editor.read().active().is_some();
            let listing = loops_open();
            let summoned = palette.read().is_some();
            let handle = pane.borrow().clone();
            if let Some(handle) = handle
                && (!editing || listing)
                && !summoned
            {
                // a headless refusal has no one to tell; the caret simply
                // stays where it was
                spawn(async move {
                    let _ = handle.set_focus(true).await;
                });
            }
        }
    });

    // the follow's landing half, from a caret already in hand — the palette
    // runs this against the offset it froze at open, where a live probe
    // would answer `null` because its own input took the focus
    let follow_at = use_callback(move |units: usize| {
        let target = {
            let editor = editor.peek();
            editor.active_source().and_then(|slice| {
                links::link_at(
                    slice,
                    blocks::byte_offset_of_utf16(slice, units),
                )
            })
        };
        // only time notes have somewhere to open; the rest wait for
        // v1's table (adr/2026-07-permanent-notes-wait-for-table.md)
        if let Some(target) = target
            && let Some(scale) = links::scale_of(&target, &notes.peek())
        {
            select.call((scale, target));
        }
    });

    // one follow path for Ctrl+Enter and for a Ctrl+click in the source:
    // both ask the widget where the caret is and go wherever it is standing
    // (adr/2026-08-ctrl-enter-opens-time-links.md)
    let follow_link = use_callback({
        let probe = probe.clone();
        move |()| {
            let Some(probe) = probe.clone() else { return };
            spawn(async move {
                // the probe answers `null` unless a textarea has focus, so
                // a note with no active block stops here
                let Some(units) = (probe.0)().await else {
                    return;
                };
                follow_at.call(units);
            });
        }
    });

    // one accept path for Enter and for a click on a row; the anchor comes
    // from the render that drew the row, so nothing here has to look it up
    let accept = use_callback({
        let pending_caret = pending_caret.clone();
        move |(anchor, link_id): (usize, String)| {
            let text = links::format_link(&link_id);
            editor.write().insert(anchor, &text);
            pending_caret.set(Some(anchor + text.encode_utf16().count()));
            picker.set(None);
            epoch += 1;
        }
    });

    // the capture chord's working half, lifted so the palette runs the same
    // body; the chord's own guard (no active block) stays in its match arm,
    // because it exists to keep one keystroke from being both paste and
    // capture — which a palette run cannot be
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

    // the picker's opening half, from an anchor already in hand — Ctrl+L
    // probes and then lands here; the palette lands here with the caret it
    // froze at open (adr/2026-08-command-palette-overlay-shape.md)
    let open_picker_at = use_callback({
        let root = root.clone();
        move |anchor: usize| match completions(&root) {
            Ok(entries) => {
                query.set(String::new());
                highlighted.set(0);
                picker.set(Some(Picker { anchor, entries }));
            }
            Err(msg) => editor.write().set_notice(msg),
        }
    });

    // closing the palette puts the focus back itself, because nothing else
    // will: with no block active the effect above re-takes the pane, and
    // with one active a remount with the frozen caret pending re-takes the
    // textarea — the accepted completion's machinery
    // (adr/2026-08-command-palette-overlay-shape.md)
    let close_palette = use_callback({
        let pending_caret = pending_caret.clone();
        move |restore: bool| {
            let caret = (*palette.peek()).and_then(|open| open.caret);
            palette.set(None);
            if restore && editor.peek().active().is_some() {
                pending_caret.set(caret);
                epoch += 1;
            }
        }
    });

    // one run path for Enter and for a click on a row. Focus settles first,
    // then the command runs — except the two that must keep it: `insert
    // link`'s picker owns the focus it just took, and `follow link`
    // replaces the editor wholesale, where a stale pending caret would leak
    // into the next note's textarea. Exhaustive on purpose: a CommandId
    // added without wiring does not compile
    // (adr/2026-08-palette-birth-command-list.md).
    let run_command =
        use_callback(move |(frozen, id): (Palette, palette::CommandId)| {
            let restores = !matches!(
                id,
                palette::CommandId::InsertLink
                    | palette::CommandId::FollowLink
            );
            close_palette.call(restores);
            match id {
                palette::CommandId::ToggleTheme => {
                    root_commands.toggle_theme.call(());
                }
                palette::CommandId::Quit => root_commands.quit.call(()),
                palette::CommandId::CaptureClipboard => {
                    capture_clipboard.call(());
                }
                // the caret commands run against the frozen offset; when
                // the open froze none (a headless run without a probe),
                // they quietly decline — the chords' own guard idiom
                palette::CommandId::InsertLink => {
                    if let Some(anchor) = frozen.caret {
                        open_picker_at.call(anchor);
                    }
                }
                palette::CommandId::FollowLink => {
                    if let Some(units) = frozen.caret {
                        follow_at.call(units);
                    }
                }
                palette::CommandId::PreviousMonth => page.call(false),
                palette::CommandId::NextMonth => page.call(true),
                palette::CommandId::OpenLoops => toggle_loops.call(()),
                palette::CommandId::GoToToday => go_today.call(()),
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
    let panes =
        block_panes(&editor.read(), &root, theme, &mut fragments.borrow_mut());
    let notice = editor.read().notice().map(str::to_string);
    let footer = link_footer(&root, &editor.read(), &id, &note_list);
    // the matches are cloned out of the picker so rsx borrows nothing from
    // the signal it also writes
    let open_picker = picker.read().as_ref().map(|open| {
        let matches: Vec<links::Completion> =
            links::filter(&open.entries, &query.read())
                .into_iter()
                .cloned()
                .collect();
        (open.anchor, matches)
    });
    // the palette's rows, cloned out the same way; which commands exist at
    // all was decided at open (adr/2026-08-palette-birth-command-list.md)
    let open_palette = palette().map(|frozen| {
        let matches = palette::filter(
            &palette_query.read(),
            palette::Context {
                block_active: frozen.block_active,
            },
        );
        (frozen, matches)
    });
    let captured = (exists && scale == NoteType::Daily)
        .then(|| captured_lines(&root, &id));
    let day_ids: HashSet<&str> =
        note_list.iter().map(|(note, _)| note.as_str()).collect();
    let weeks = logs::month_grid(month());
    let seasons = logs::season_row(month());

    let keyboard = {
        let root = root.clone();
        let fragments = fragments.clone();
        let probe = probe.clone();
        move |event: KeyboardEvent| {
            match event.key() {
                // the open-loops list is a destination you leave; escape
                // reaches here only when no block owns it
                Key::Escape if loops_open() => loops_open.set(false),
                // in-app capture: what is on the clipboard becomes a note in
                // capture/, no required fields, nothing to fill in
                // (adr/2026-08-capture-headless-second-process.md). Shift
                // uppercases the character the browser reports, so the chord
                // is matched either way. Over an active block it stays the
                // webview's own paste — capturing into a file you are not
                // looking at, *and* pasting into the one you are, would be
                // two actions on one keystroke.
                Key::Character(ref character)
                    if character.eq_ignore_ascii_case("v")
                        && event.modifiers().ctrl()
                        && event.modifiers().shift()
                        && editor.peek().active().is_none() =>
                {
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
                    let Some(probe) = probe.clone() else { return };
                    spawn(async move {
                        let Some(anchor) = (probe.0)().await else {
                            return;
                        };
                        open_picker_at.call(anchor);
                    });
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
                        && picker.peek().is_none() =>
                {
                    // the webview answers a bare Ctrl+P with a print dialog
                    event.prevent_default();
                    let probe = probe.clone();
                    spawn(async move {
                        let block_active = editor.peek().active().is_some();
                        let caret = match probe {
                            Some(probe) if block_active => (probe.0)().await,
                            _ => None,
                        };
                        palette_query.set(String::new());
                        palette_highlighted.set(0);
                        palette.set(Some(Palette {
                            block_active,
                            caret,
                        }));
                    });
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
                    follow_link.call(());
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

    rsx! {
        Chrome {
            screen: Screen::Logs,
            loops: loops.read().len(),
            on_ember: move |_| toggle_loops.call(()),
        }
        div {
            class: "logs",
            // the enter-to-create keystroke lands here and the theme/quit
            // chords bubble on up to the .app root — which is why the pane
            // takes focus back whenever no block holds it
            tabindex: "0",
            autofocus: true,
            onmounted: move |event: Event<MountedData>| {
                pane.borrow_mut().replace(event.data());
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
                // the palette floats (position: fixed), so it leads the
                // pane in source without displacing anything on screen
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
                                            Key::Escape => close_palette.call(true),
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
                    match panes {
                        Some(panes) => rsx! {
                            div { class: "note-blocks",
                                for pane in panes {
                                    {
                                        match pane {
                                            Pane::Source { start, text } => {
                                                let rows = text.split('\n').count();
                                                rsx! {
                                                    textarea {
                                                        // the epoch remounts it after a link is
                                                        // spliced in, so the uncontrolled value is
                                                        // rebuilt from the buffer
                                                        key: "{start}-{epoch}",
                                                        class: "block-active",
                                                        rows: "{rows}",
                                                        spellcheck: "false",
                                                        // autofocus only applies at document load in
                                                        // the webview: a swapped-in textarea asks for
                                                        // its own focus, and a refusal has no one to
                                                        // tell — the caret simply stays where it was
                                                        onmounted: {
                                                            let pending_caret = pending_caret.clone();
                                                            let writer = writer.clone();
                                                            move |event: Event<MountedData>| {
                                                                let caret = pending_caret.take();
                                                                let writer = writer.clone();
                                                                async move {
                                                                    let _ = event.set_focus(true).await;
                                                                    // after an accepted completion, the
                                                                    // caret belongs past the link, not at
                                                                    // whatever the webview picks
                                                                    if let Some(units) = caret
                                                                        && let Some(writer) = writer
                                                                    {
                                                                        (writer.0)(units).await;
                                                                    }
                                                                }
                                                            }
                                                        },
                                                        initial_value: "{text}",
                                                        oninput: move |event| {
                                                            editor.write().edit(&event.value());
                                                        },
                                                        // Ctrl+click follows the link it lands
                                                        // in, like Ctrl+Enter: the click has
                                                        // already moved the caret, so the same
                                                        // probe answers where
                                                        onclick: move |event: MouseEvent| {
                                                            if event.modifiers().ctrl() {
                                                                follow_link.call(());
                                                            }
                                                        },
                                                        onkeydown: {
                                                            let fragments = fragments.clone();
                                                            let probe = probe.clone();
                                                            move |event: KeyboardEvent| {
                                                                let key = event.key();
                                                                if key == Key::Escape {
                                                                    editor.write().deactivate();
                                                                    fragments.borrow_mut().sweep();
                                                                } else if key == Key::ArrowUp
                                                                    || key == Key::ArrowDown
                                                                {
                                                                    // a vertical arrow may leave the block:
                                                                    // ask the webview where the caret is —
                                                                    // the browser default on the edge lines
                                                                    // is a no-op, so the async probe races
                                                                    // nothing. It must still not page the
                                                                    // month grid below.
                                                                    event.stop_propagation();
                                                                    if let Some(probe) = &probe {
                                                                        let probe = probe.clone();
                                                                        let fragments = fragments.clone();
                                                                        let up = key == Key::ArrowUp;
                                                                        spawn(async move {
                                                                            if let Some(units) = (probe.0)().await {
                                                                                editor.write().slide(units, up);
                                                                                fragments.borrow_mut().sweep();
                                                                            }
                                                                        });
                                                                    }
                                                                } else if !event.modifiers().ctrl() {
                                                                    // the rest belongs to the caret: keep
                                                                    // enter off the create handler below;
                                                                    // the ctrl chords still bubble to the
                                                                    // app root
                                                                    event.stop_propagation();
                                                                }
                                                            }
                                                        },
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
                        },
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
                {
                    match open_picker {
                        Some((anchor, matches)) => {
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
                                            Key::Escape => picker.set(None),
                                            Key::Enter => {
                                                // no matches: the keystroke does
                                                // nothing rather than guessing
                                                if let Some(entry) = matches.get(highlighted()) {
                                                    accept.call((anchor, entry.id.clone()));
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
                                            move |_| accept.call((anchor, id.clone()))
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
                                                        // only a time note has somewhere to
                                                        // open in v0; the rest are visible
                                                        // but inert until v1's table
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
                                                        None => rsx! {
                                                            span {
                                                                class: "link-entry",
                                                                class: if link.dangling { "link-dangling" },
                                                                "{link.label}"
                                                            }
                                                        },
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
}

/// The table mounts in v1; until then its icon stays dim and neither icon
/// navigates — the logs are the only screen.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Table,
    Logs,
}

/// The one-line chrome (design § Chrome): two 14×14 stroked icons, the
/// current screen's lit, and the open-loops ember. Zero loops renders
/// nothing at all — absence, not a zero — so the ember is clickable exactly
/// when there is a list to show (adr/2026-08-loops-list-overlay.md).
#[component]
fn Chrome(
    screen: Screen,
    loops: usize,
    on_ember: EventHandler<()>,
) -> Element {
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
                span {
                    class: "ember",
                    onclick: move |_| on_ember.call(()),
                    "{loops}"
                }
            }
        }
    }
}

/// What one look at the index yields: the rail's time notes, and the open
/// loops themselves rather than a count of them.
type Survey = (Vec<(String, NoteType)>, Vec<String>);

/// What the shell mounts with: the vault root and that survey.
type Loaded = (PathBuf, Vec<(String, NoteType)>, Vec<String>);

fn load(root: Option<PathBuf>) -> Result<Loaded, String> {
    match root {
        Some(root) => match load_notes(&root) {
            Ok((notes, loops)) => Ok((root, notes, loops)),
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
    Ok((index.time_notes()?, open_loops(index)?))
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

/// The open link picker's fixed half. `anchor` is the caret Ctrl+L froze, in
/// UTF-16 code units within the active block; `entries` is the index
/// snapshot the query filters, taken once at open because nothing can change
/// it while the popup holds focus (adr/2026-08-ctrl-l-link-picker.md). The
/// moving half — query and highlight — lives in its own signals.
#[derive(Clone, PartialEq)]
struct Picker {
    anchor: usize,
    entries: Vec<links::Completion>,
}

/// The open command palette's fixed half — the `Picker.anchor` idiom.
/// `block_active` decides which commands exist at all
/// (adr/2026-08-palette-birth-command-list.md); `caret` is the offset the
/// caret commands run against, probed at open because by dispatch time the
/// palette's own input holds the focus and the probe would answer `null`
/// (adr/2026-08-command-palette-overlay-shape.md). The moving half — query
/// and highlight — lives in its own signals, like the picker's.
#[derive(Clone, Copy, PartialEq)]
struct Palette {
    block_active: bool,
    caret: Option<usize>,
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
    /// mounted-app doc): registration runs jump-panel first — the header's
    /// ‹ today › buttons, the three seasons, then each grid row as gutter +
    /// day cells — then the note's two link-footer entries, the centre's two
    /// inactive blocks (today's preamble and heading), the two crumb jumps,
    /// and finally the five rail rows top to bottom.
    const CAL_BACK: usize = 0;
    const CAL_TODAY: usize = 1;
    const CAL_FORWARD: usize = 2;
    const SEASON_AUTUMN: usize = 5;
    const GUTTER_W31: usize = 36;
    const FOOTER_BACKLINK: usize = 42;
    const FOOTER_OUTGOING: usize = 43;
    const BLOCK_PREAMBLE: usize = 44;
    const BLOCK_HEADING: usize = 45;
    const CRUMB_WEEK: usize = 46;
    const RAIL_SUMMER: usize = 48;
    const RAIL_W30: usize = 49;
    const RAIL_DAY_23: usize = 50;
    const RAIL_DAY_22: usize = 51;
    const RAIL_DAY_21: usize = 52;
    /// July 2026 leads with two blanks, so a date's cell index is offset by
    /// one gutter per started week row.
    const fn day_cell(day: usize) -> usize {
        6 + (day + 1) / 7 + day
    }
    /// Which keydown listener is the logs pane's (the other is the root).
    const LOGS_KEYS: usize = 1;
    /// The ember registers ahead of everything in the pane, so in a vault
    /// with open loops it takes click listener 0 and every index above
    /// shifts by one — which is why the base `temp_vault` has none.
    const EMBER: usize = 0;

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
    fn leaving_a_block_hands_focus_back_to_the_pane() {
        // the window's chords only reach the app root by bubbling, so
        // something inside the app must hold focus; a textarea that
        // unmounts takes it out of the app entirely
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let clicks = listeners(&mutations, "click");
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );
        let taken = focused.load(Ordering::SeqCst);

        // editing: the block owns focus, the pane leaves it alone
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        block_on(settle(&mut dom));
        assert_eq!(focused.load(Ordering::SeqCst), taken);

        // and on the way out the pane takes it back
        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        block_on(settle(&mut dom));
        assert!(
            focused.load(Ordering::SeqCst) > taken,
            "the pane asked for focus once the block let go"
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
        let (input, _) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        // typed but inside the quiet window: only the flush can save it
        type_into(&mut dom, input, "= presque perdu\n");
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
        let (input, _) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
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
        let (input, _) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        type_and_settle(&mut dom, input, "= autosauvé\n");

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
        let (input, _) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

        let file = vault.path().join("time/2026-07-23.typ");
        let mut permissions = std::fs::metadata(&file)
            .expect("the note exists")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&file, permissions)
            .expect("the note is made read-only");

        // the settle loop spans several autosave restarts, so the
        // value-gated write is exercised on both of its sides here
        type_and_settle(&mut dom, input, "= en panne\n");
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
        rsx! { Chrome { screen, loops: 0, on_ember: move |()| {} } }
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
            53,
            "3 header + 3 seasons + 5 gutters + 31 days + 2 footer links \
             + 2 blocks + 2 crumbs + 5 rail: {html}"
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
        let html = dioxus_ssr::render(&dom);
        // the broken `#let x = (` block shows its diagnostic inline while
        // the preamble block still renders — per-block honesty
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains(RENDERED_NOTE), "{html}");
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
    fn clicking_a_block_opens_its_source_in_place() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let mutations = click_for_mutations(&mut dom, clicks[BLOCK_HEADING]);
        // the renderer announces the mount and the textarea asks for focus;
        // the fake backing refuses, which is all the handler has to absorb
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        let html = dioxus_ssr::render(&dom);

        assert!(html.contains("block-active"), "{html}");
        assert!(html.contains("= 2026-07-23"), "the raw source: {html}");
        assert!(
            html.contains(RENDERED_NOTE),
            "the preamble block stays rendered: {html}"
        );
    }

    #[test]
    fn typing_updates_the_buffer_and_escape_writes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

        type_into(&mut dom, input, "= renamed\n\nencore\n");
        let file = vault.path().join("time/2026-07-23.typ");
        let untouched =
            std::fs::read_to_string(&file).expect("the note is readable");
        assert!(!untouched.contains("renamed"), "typing alone never writes");

        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("block-active"), "escape closes it: {html}");
        // the blank line split the heading block: preamble + two prose
        assert_eq!(html.matches(r#"class="block""#).count(), 3, "{html}");
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

        // activation flushes before moving, and the failure is the notice
        click(&mut dom, clicks[BLOCK_HEADING]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("2026-07-23.typ"), "{html}");
    }

    #[test]
    fn caret_keys_stay_in_the_source_while_ctrl_chords_escape_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

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
        let (input, _) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        type_into(&mut dom, input, "= renamed\n");

        // the still-rendered preamble block is the first click target again
        let mutations = click_for_mutations(&mut dom, clicks[BLOCK_PREAMBLE]);
        assert_eq!(
            listeners(&mutations, "input").len(),
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
        let (mut dom, clicks, caret) =
            probe_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

        // caret on the heading's first line: up slides into the preamble
        *caret.lock().expect("the probe cell never poisons") = Some(0);
        press(&mut dom, keys, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "the preamble source: {html}");
        assert!(!html.contains("= 2026-07-23"), "one active block: {html}");
    }

    #[test]
    fn a_mid_block_caret_or_an_empty_probe_slides_nowhere() {
        let vault = temp_vault();
        let (mut dom, clicks, caret) =
            probe_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_PREAMBLE]);

        // the probe answered nothing (no textarea focused)
        press(&mut dom, keys, Key::ArrowDown, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "still the preamble: {html}");

        // a caret with newlines on both sides is ordinary movement
        *caret.lock().expect("the probe cell never poisons") =
            Some("#import \"/templates/template.typ\": *\n#s".len());
        press(&mut dom, keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, keys, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "still the preamble: {html}");
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
        with_reactor(|| dom.rebuild_to_vec());
        (dom, sender)
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
        let (mut dom, sender) = watched_app(Some(vault.path().to_path_buf()));
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
        let (mut dom, sender) = watched_app(Some(vault.path().to_path_buf()));
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
        let (mut dom, sender) = watched_app(Some(vault.path().to_path_buf()));
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
        let (mut dom, sender) = watched_app(Some(vault.path().to_path_buf()));
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
        let (mut dom, sender) = watched_app(Some(vault.path().to_path_buf()));
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
        let (mut dom, sender) = watched_app(Some(vault.path().to_path_buf()));
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
    fn over_an_active_block_the_chord_stays_an_ordinary_paste() {
        let vault = temp_vault();
        let captures = vault.path().join("capture");
        let before = std::fs::read_dir(&captures)
            .expect("the capture directory is there")
            .count();
        // a clipboard with something on it, but a block is being edited:
        // the webview pastes into it and nothing is captured
        let (mut dom, clicks, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Some("pour le bloc".to_string()),
            Some(CAPTURED_AT),
        );
        activate_block(&mut dom, clicks[BLOCK_HEADING]);
        capture_chord(&mut dom, &keys);
        assert_eq!(
            std::fs::read_dir(&captures)
                .expect("the capture directory is there")
                .count(),
            before
        );
    }

    // -- the link picker: Ctrl+L, filter, accept -----------------------------

    #[test]
    fn ctrl_l_opens_the_picker_and_enter_writes_the_link() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, written) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

        // the caret sits right after "= 2026-07-23\n"
        let anchor = "= 2026-07-23\n".len();
        *caret.lock().expect("the probe cell never poisons") = Some(anchor);
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

        let mutations = press_for_mutations(
            &mut dom,
            picker_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "accepting closes it: {html}");
        assert!(
            source_of(&dom).contains(r#"#l("2026-summer")"#),
            "spliced at the caret: {}",
            source_of(&dom)
        );
        // the accept remounted the textarea; the renderer then announces it,
        // which is when the caret is put back
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        assert_eq!(
            *written.lock().expect("the writer cell never poisons"),
            vec![anchor + r#"#l("2026-summer")"#.len()],
            "the caret lands past the link it just wrote"
        );
    }

    #[test]
    fn a_row_click_accepts_the_completion_too() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);

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
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);
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
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);
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
    fn the_theme_chord_still_works_over_an_open_picker() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);
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
    fn ctrl_l_needs_an_active_block_a_probe_and_a_closed_picker() {
        let vault = temp_vault();

        // no active block: the caret is nowhere to anchor to
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys, _) = pane_targets(&mut dom, &clicks);
        press(&mut dom, keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");

        // a probe that answers nothing: no anchor, no picker
        let (_, block_keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        press(&mut dom, block_keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");

        // open, then a second Ctrl+L leaves the first one alone
        *caret.lock().expect("the probe cell never poisons") = Some(0);
        let (input, _) = open_picker(&mut dom, block_keys);
        type_into(&mut dom, input, "summer");
        press(&mut dom, block_keys, ctrl_l(), Modifiers::CONTROL);
        assert_eq!(
            picker_ids(&dom),
            vec!["2026-summer"],
            "the second chord left the open picker alone"
        );

        // and without an injected probe at all, the chord is inert
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        press(&mut dom, keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");
    }

    // -- Ctrl+Enter: following the link under the caret ---------------------

    /// The heading block of the fixture's selected day is
    /// `= 2026-07-23\n#l("2026-07-22")`, so the link opens at this offset.
    const LINK_IN_HEADING: usize = "= 2026-07-23\n".len();

    #[test]
    fn ctrl_enter_opens_the_time_note_under_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

        // inside the `#l("2026-07-22")` the heading block ends with
        *caret.lock().expect("the probe cell never poisons") =
            Some(LINK_IN_HEADING + 3);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the chord jumped to the linked day: {html}"
        );
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn ctrl_clicking_a_link_in_the_source_opens_it_too() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let mutations = click_for_mutations(&mut dom, clicks[BLOCK_HEADING]);
        let source = listeners(&mutations, "click")[0];

        // the click that follows has already moved the caret into the link
        *caret.lock().expect("the probe cell never poisons") =
            Some(LINK_IN_HEADING + 3);
        ctrl_click(&mut dom, source);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the ctrl+click jumped to the linked day: {html}"
        );
        assert!(html.contains(RENDERED_NOTE), "{html}");
    }

    #[test]
    fn a_plain_click_in_the_source_only_moves_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let mutations = click_for_mutations(&mut dom, clicks[BLOCK_HEADING]);
        let source = listeners(&mutations, "click")[0];

        // the caret is in the link, but without the modifier nothing follows
        *caret.lock().expect("the probe cell never poisons") =
            Some(LINK_IN_HEADING + 3);
        click(&mut dom, source);
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
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

        // in the heading text, well before the link
        *caret.lock().expect("the probe cell never poisons") = Some(2);
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
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(20)]);
        let (_, keys, _) = pane_targets(&mut dom, &clicks);

        // the probe answers nothing, the way it does with no textarea focused
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        // and again with an answer, which no block can be measured against
        *caret.lock().expect("the probe cell never poisons") = Some(0);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        assert!(
            !vault.path().join("time/2026-07-20.typ").exists(),
            "the chord is not the create keystroke"
        );

        // and with no probe injected at all it is simply inert
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">23"), "{html}");
    }

    #[test]
    fn an_index_that_cannot_list_notes_becomes_the_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);
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
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);
        replace_database_with_a_directory(vault.path());

        press(&mut dom, keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("links:"), "{html}");
    }

    #[test]
    fn accepting_without_a_caret_writer_still_splices() {
        // `main` always injects one; a renderer that cannot move the caret
        // must still not lose the link
        let vault = temp_vault();
        let caret = Arc::new(std::sync::Mutex::new(Some(0usize)));
        let feed = caret.clone();
        let probe = CaretProbe(Arc::new(move || {
            let units = *feed.lock().expect("the probe cell never poisons");
            Box::pin(async move { units })
        }));
        let (mut dom, mutations) = mounted_app_with_probe(
            Some(vault.path().to_path_buf()),
            None,
            Some(probe),
            None,
        );
        let clicks = listeners(&mutations, "click");
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        let (input, picker_keys) = open_picker(&mut dom, keys);
        type_into(&mut dom, input, "summer");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        assert!(
            source_of(&dom).contains(r#"#l("2026-summer")"#),
            "{}",
            source_of(&dom)
        );
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
    fn a_backlink_from_a_permanent_note_is_visible_but_inert() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/alpha.typ"),
            linking(note("alpha"), "2026-07-23"),
        )
        .expect("alpha is rewritten");
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"<span class="link-entry ">alpha</span>"#),
            "no jump, no dangling mark: {html}"
        );
    }

    #[test]
    fn a_link_to_nothing_is_marked_as_it_is_typed() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, _) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        // the heading block loses its outgoing link and gains a ghost one
        type_and_settle(&mut dom, input, "= 2026-07-23\n#l(\"fantôme\")\n");
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
        let (input, _) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        type_and_settle(&mut dom, input, "= 2026-07-23\n");
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
                "previous month",
                "next month",
                "open loops",
                "go to today",
            ],
            "no block active: the caret commands are hidden"
        );

        type_into(&mut dom, input, "THEME");
        assert_eq!(
            palette_labels(&dom),
            vec!["toggle theme"],
            "the query filters, ignoring case"
        );
    }

    #[test]
    fn over_an_active_block_all_nine_commands_are_listed() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);
        open_palette(&mut dom, keys);
        let labels = palette_labels(&dom);
        assert_eq!(labels.len(), 9, "{labels:?}");
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
    fn the_palette_runs_insert_link_at_the_frozen_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, written) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") =
            Some(LINK_IN_HEADING);
        let (input, palette_keys) = open_palette(&mut dom, keys);
        // the palette holds the offset it froze; a live probe would now
        // answer nothing, its own input having taken the focus
        *caret.lock().expect("the probe cell never poisons") = None;

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
        let accept = press_for_mutations(
            &mut dom,
            picker_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        assert!(
            source_of(&dom).contains(r#"#l("2026-summer")"#),
            "spliced at the frozen anchor: {}",
            source_of(&dom)
        );
        mount(&mut dom, listeners(&accept, "mounted")[0]);
        assert_eq!(
            *written.lock().expect("the writer cell never poisons"),
            vec![LINK_IN_HEADING + r#"#l("2026-summer")"#.len()],
            "the caret lands past the link, from the frozen offset"
        );
    }

    #[test]
    fn the_palette_runs_follow_link_from_the_frozen_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") =
            Some(LINK_IN_HEADING + 3);
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
    fn the_caret_commands_decline_without_a_frozen_caret() {
        // a probe that answers nothing froze no caret: both commands are
        // listed (a block is active) but quietly decline
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "insert link");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");
        assert!(!html.contains("command-palette"), "closed all the same");

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "follow");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "the selection stayed put: {html}"
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
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );

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
    fn escape_over_a_block_remounts_the_textarea_with_the_frozen_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, caret, written) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") =
            Some(LINK_IN_HEADING);

        let (_, palette_keys) = open_palette(&mut dom, keys);
        let mutations = press_for_mutations(
            &mut dom,
            palette_keys,
            Key::Escape,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "still editing: {html}");
        // the textarea came back; its mount puts the caret where Ctrl+P
        // froze it
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        assert_eq!(
            *written.lock().expect("the writer cell never poisons"),
            vec![LINK_IN_HEADING],
            "the caret went back where the palette found it"
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
        let (mut dom, clicks, caret, _) =
            probe_and_writer_app(Some(vault.path().to_path_buf()));
        let (_, block_keys) = activate_block(&mut dom, clicks[BLOCK_HEADING]);
        *caret.lock().expect("the probe cell never poisons") = Some(0);
        open_picker(&mut dom, block_keys);
        press(&mut dom, block_keys, ctrl_p(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command-palette"), "{html}");
        assert!(html.contains("link-picker"), "{html}");
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
        mounted_app_with_probe(root, closer, None, None)
    }

    fn mounted_app_with_probe(
        root: Option<PathBuf>,
        closer: Option<Closer>,
        probe: Option<CaretProbe>,
        writer: Option<CaretWriter>,
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
        if let Some(probe) = probe {
            dom.insert_any_root_context(Box::new(probe));
        }
        if let Some(writer) = writer {
            dom.insert_any_root_context(Box::new(writer));
        }
        let mutations = dom.rebuild_to_vec();
        (dom, mutations)
    }

    /// Like `rendered_app`, but with a scripted caret probe injected: each
    /// arrow press reads whatever the returned cell holds at that moment.
    fn probe_app(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        Arc<std::sync::Mutex<Option<usize>>>,
    ) {
        let (dom, clicks, caret, _) = probe_and_writer_app(root);
        (dom, clicks, caret)
    }

    /// The link picker's harness: a scripted probe feeding it an anchor and
    /// a recording caret writer, so both halves of the round trip are
    /// observable.
    #[allow(clippy::type_complexity)]
    fn probe_and_writer_app(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        Arc<std::sync::Mutex<Option<usize>>>,
        Arc<std::sync::Mutex<Vec<usize>>>,
    ) {
        let caret = Arc::new(std::sync::Mutex::new(None::<usize>));
        let feed = caret.clone();
        let probe = CaretProbe(Arc::new(move || {
            let units = *feed.lock().expect("the probe cell never poisons");
            Box::pin(async move { units })
        }));
        let written = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = written.clone();
        let writer = CaretWriter(Arc::new(move |units| {
            recorder
                .lock()
                .expect("the writer cell never poisons")
                .push(units);
            Box::pin(async {})
        }));
        let (dom, mutations) =
            mounted_app_with_probe(root, None, Some(probe), Some(writer));
        let clicks = listeners(&mutations, "click");
        (dom, clicks, caret, written)
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

    /// A click with Ctrl held — the chord that follows a link in the source.
    fn ctrl_click(dom: &mut VirtualDom, target: ElementId) {
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
                "click",
                Event::new(data, true),
                target,
            );
            dom.process_events();
            let _ = dom.render_immediate_to_vec();
        })
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

    /// Types and lets the debounced autosave finish, so assertions see the
    /// settled state rather than a half-run timer.
    fn type_and_settle(dom: &mut VirtualDom, target: ElementId, text: &str) {
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

    /// The picker's rows, in order — the assertions want the list itself,
    /// not a substring of a page that also holds the rail.
    fn picker_ids(dom: &VirtualDom) -> Vec<String> {
        dioxus_ssr::render(dom)
            .split(r#"<span class="picker-id">"#)
            .skip(1)
            .filter_map(|rest| rest.split('<').next().map(str::to_string))
            .collect()
    }

    /// The active textarea's text, unescaped — the rendered page HTML-escapes
    /// the quotes an `#l(..)` call is made of.
    fn source_of(dom: &VirtualDom) -> String {
        dioxus_ssr::render(dom)
            .split(r#"initial_value=""#)
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default()
            .replace("&#34;", "\"")
    }

    /// The link chord, spelled once.
    fn ctrl_l() -> Key {
        Key::Character("l".into())
    }

    /// The palette chord, spelled once.
    fn ctrl_p() -> Key {
        Key::Character("p".into())
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
        // the pane keydown target is still the one from the initial mount
        let mutations = click_for_mutations(dom, clicks[RAIL_DAY_23]);
        let keys = listeners(&mutations, "keydown");
        let target = *keys.last().unwrap_or(&clicks[0]);
        (clicks[0], target, target)
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
    fn activate_block(
        dom: &mut VirtualDom,
        block: ElementId,
    ) -> (ElementId, ElementId) {
        let mutations = click_for_mutations(dom, block);
        let inputs = listeners(&mutations, "input");
        let keys = listeners(&mutations, "keydown");
        (inputs[0], keys[0])
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
            _: &PlatformEventData,
        ) -> CompositionData {
            unreachable!("the shell never listens for this event")
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

        fn convert_resize_data(&self, _: &PlatformEventData) -> ResizeData {
            unreachable!("the shell never listens for this event")
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
