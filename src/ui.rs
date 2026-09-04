use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::future::Future;
use std::ops::Range;
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
use crate::compute::{self, ComputeFeed, Job, Outcome};
use crate::domain::{NoteCategory, NoteType};
use crate::editor::{Deletion, Editor};
use crate::index::{Index, TableNote};
use crate::keymap;
use crate::links;
use crate::logs::{self, Selection};
use crate::markup;
use crate::motions::{self, Lines, Motion};
use crate::palette;
use crate::positions::Positions;
use crate::render::{
    BodyCache, BodyView, DEFAULT_SIZE, FragmentCache, FragmentView,
    RenderTheme,
};
use crate::shown::Shown;
use crate::status::{Liveness, Notice, Source, Status};
use crate::table;
use crate::time;
use crate::undo;
use crate::vim;
use crate::watch;

/// One idle timer drives the save (adr/2026-07-debounced-autosave.md);
/// shortened under `cfg(test)` so the settled state is a few polls away.
#[cfg(not(test))]
const QUIET: Duration = Duration::from_millis(500);
#[cfg(test)]
const QUIET: Duration = Duration::from_millis(1);

/// The settings overlay's font-size stepper bounds, 2px per press: below
/// `MIN_FONT_SIZE` prose is unreadable, above `MAX_FONT_SIZE` a line stops
/// fitting the pane at ordinary widths (adr/2026-08-settings-overlay.md).
const MIN_FONT_SIZE: u16 = 12;
const MAX_FONT_SIZE: u16 = 28;
const FONT_STEP: u16 = 2;

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

/// Where a whole `[count]j`/`k` run ended: the landing span's `data-start`,
/// the UTF-16 offset within its text node, the pixel x the walk held — the
/// goal column, resolved by the only thing that knows where the lines wrap
/// — and how many of the asked steps the walk actually took before the
/// drawn lines ran out.
/// Whether an IME composition owns the keyboard. Input state, unlike the
/// drawn preview: it is true in every mode, because the commit keystroke
/// can reach the sink unflagged and the grammar would read it as a real
/// key (adr/2026-08-hidden-ime-sink.md).
///
/// `Closing` is the grace WebKitGTK's doubled end forces: it fires an
/// empty `compositionend` before the real one and can slip a stray
/// keydown between them, so the flag cannot drop on the empty end. The
/// grace is exactly one keystroke, so a composition that genuinely ends
/// empty — an abort — costs one key and never wedges the editor.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Composing {
    No,
    Open,
    Closing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    pub start: usize,
    pub units: usize,
    pub x: f64,
    pub taken: usize,
}

/// What the line probe answers: one landing for the whole run, or `None`
/// when it could not take even one step.
pub type Walk = Pin<Box<dyn Future<Output = Option<Landing>>>>;

/// How j and k find the lines the webview actually drew: `main` injects a
/// `caretPositionFromPoint` walk that takes every step of the run inside
/// the webview, the headless tests inject a scripted fake — the `HitProbe`
/// pattern (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
///
/// The whole run rides one round trip because `dioxus::document::eval`
/// sends its script the moment it is constructed: a step-per-eval walk
/// would build the next probe in the same task poll as the previous
/// landing, before the DOM flushed it, and read the caret's pre-move rect.
/// The walk therefore steps from the rect of the character it just landed
/// on, never from the app-drawn caret element, which is stale after step
/// one. `j`/`k` edit no text, so the spans it measures stay valid.
#[derive(Clone)]
pub struct LineProbe(
    pub Arc<dyn Fn(Option<f64>, bool, usize) -> Walk + Send + Sync>,
);

/// The column a `j`/`k` run holds across its steps: the pixel x the line
/// probe resolved and the logical cluster column the degraded fallback
/// keeps, both forgotten together by every key and every mouse-driven
/// caret move that is not part of the run
/// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
///
/// `generation` stamps the run: a walk task still awaiting its probe when
/// another key forgets the run must move nothing once it resolves —
/// neither the caret, which that key has already moved, nor the goal
/// column, which now belongs to a fresher run — so it checks the stamp it
/// started with before touching either.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Goal {
    x: Option<f64>,
    column: Option<usize>,
    generation: u64,
}

impl Goal {
    /// Forgetting the run: both columns go, and the bump disowns any goal
    /// a walk still in flight is about to answer with.
    fn forgotten(self) -> Self {
        Goal {
            x: None,
            column: None,
            generation: self.generation.wrapping_add(1),
        }
    }
}

/// One clipboard write, done when the future resolves.
pub type Written = Pin<Box<dyn Future<Output = ()>>>;

/// One block's caret-laid-out lines, each piece tagged with the markup role
/// it renders under — or untagged (`None`) on the Typst verdict, where the
/// rsx below omits the class attribute outright, leaving the fallback's
/// markup byte-for-byte what it was before this block drew styled
/// (adr/2026-08-css-draws-the-markup.md). Shared by `Pane::Source` and
/// `Pane::Selected`, which both build it from the same
/// `markup::model`/`markup::tint` pair.
type TintedLines = Vec<Vec<(Option<(markup::Role, bool)>, caret::Piece)>>;

/// How Ctrl+C reaches the system clipboard: `main` injects a JS
/// `navigator.clipboard.writeText`, the headless tests inject a recorder —
/// the `Clipboard` seam in the other direction
/// (adr/2026-08-hidden-ime-sink.md).
#[derive(Clone)]
pub struct ClipboardWrite(pub Arc<dyn Fn(String) -> Written + Send + Sync>);

/// How ordinary paste, vim's register and in-app capture read the system
/// clipboard: `main` injects the native worker, and headless tests inject a
/// scripted result. Failures carry their reason to the status surface.
#[derive(Clone)]
pub struct Clipboard(
    #[allow(clippy::type_complexity)]
    pub  Arc<
        dyn Fn() -> Pin<Box<dyn Future<Output = Result<String, String>>>>
            + Send
            + Sync,
    >,
);

/// The clipboard's image as PNG bytes, read the way its text is: `main`
/// injects the worker's image read, the headless tests a scripted answer.
/// `Ok(None)` is a clipboard holding no image
/// (adr/2026-09-an-image-pastes-into-assets.md).
#[derive(Clone)]
pub struct ClipboardImage(
    #[allow(clippy::type_complexity)]
    pub  Arc<
        dyn Fn() -> Pin<
                Box<dyn Future<Output = Result<Option<Vec<u8>>, String>>>,
            > + Send
            + Sync,
    >,
);

/// How the vault watcher reaches the screen: `main` starts the watcher on
/// its own thread and hands the receiving end over here, the headless tests
/// send batches by hand (adr/2026-08-watcher-feeds-the-ui.md). Taken out of
/// the cell once, by the shell's first render — a receiver has one owner,
/// and an app with no feed simply keeps the index it loaded at launch.
/// A watcher that would not start rides along as `trouble`, so the status
/// surface can say the vault is unwatched instead of stderr saying it to
/// nobody (adr/2026-08-status-surface-owns-notices.md).
#[derive(Clone)]
pub struct VaultFeed {
    #[allow(clippy::type_complexity)]
    pub changes:
        Arc<Mutex<Option<UnboundedReceiver<Vec<watch::VaultChange>>>>>,
    pub trouble: Option<String>,
}

/// What `main`'s pre-launch seeding of the vault skeleton had to say:
/// `None` when every default template landed or already stood, the failure
/// otherwise — carried in so the status surface can report it, a desktop
/// app's stderr being nowhere
/// (adr/2026-08-templates-seeded-from-embedded-fixtures.md).
#[derive(Clone)]
pub struct SeedTrouble(pub Option<String>);

/// The clock a capture is stamped by, injected like `Today` and read only
/// when one is written (adr/2026-08-capture-timestamp-ids.md). `Today` is
/// the date every screen is drawn from and is read once at launch; a
/// capture needs the time of day too, and needs it at the moment it
/// arrives — so this is a closure rather than a value.
#[derive(Clone)]
pub struct Now(pub Arc<dyn Fn() -> jiff::Zoned + Send + Sync>);

/// What a `#link` destination is handed to — the desktop's opener, given
/// the resolved target and answering only whether it could be started.
/// Absent in the headless tests that never follow one; the tests that do
/// inject a recorder (adr/2026-09-link-is-for-resources.md).
#[derive(Clone)]
pub struct Launcher(pub Arc<Opener>);

/// The launcher's one call: the resolved target in, whether the opener
/// could be started out.
pub type Opener = dyn Fn(&str) -> Result<(), String> + Send + Sync;

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
        // inlined, not `asset!`: manganis only resolves an asset path under
        // the `dx` CLI, so a plainly-built binary (cargo run aside) asks for
        // a bundle that does not exist and comes up unstyled
        // (adr/2026-08-theme-css-inlined.md)
        document::Style { {include_str!("../assets/theme.css")} }
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
                    && event.key() == Key::Character("q".to_string())
                {
                    quit.call(());
                }
            },
            {
                // the vault-error takeover survives only for a missing
                // root — an index that will not build is a notice and a
                // degraded glyph, never a dead screen: the .typ files are
                // intact (adr/2026-08-startup-survey-async.md)
                match &vault.0 {
                    Some(root) => {
                        rsx! { Shell { root: root.clone(), today: today.0 } }
                    }
                    None => rsx! {
                        div { class: "vault-error",
                            "no vault: define NOTE_VAULT or HOME"
                        }
                    },
                }
            }
        }
    }
}

/// The logs screen: time rail, rendered centre
/// pane with its scale chain and "captured today" block, month-grid jump
/// panel. Everything it decides comes from `logs`; the component is wiring.
#[component]
fn Shell(root: PathBuf, today: Date) -> Element {
    // the compute tier this shell submits to: `main` injects the threaded
    // adapter, the headless tests inject a scripted one or nothing and get
    // inline (adr/2026-08-compute-tier-worker-seam.md)
    let feed = use_hook(|| {
        try_consume_context::<ComputeFeed>().unwrap_or_else(compute::inline)
    });
    // the editor opens today's note by a stat, not the survey — the file
    // is the truth and the threaded launch has no survey yet
    // (adr/2026-08-startup-survey-async.md)
    let mut editor = use_signal({
        let root = root.clone();
        let id = time::day_id(today);
        move || open_selected(&root, time_note_path(&root, &id).exists(), &id)
    });
    let mut notes = use_signal(Vec::new);
    // the open loops themselves; the ember shows how many there are and the
    // overlay shows which (adr/2026-08-loops-list-overlay.md)
    let mut loops = use_signal(Vec::new);
    // the table's notes, third rider on the same survey the watcher refreshes
    let mut table_notes = use_signal(Vec::<TableNote>::new);
    // the link edges, the survey's fourth rider — the constellation redraws
    // whenever the watcher redraws the cards (adr/2026-08-edges-svg-under-cards.md)
    let mut edges = use_signal(Vec::new);
    // canvas positions, read once here and touched by nothing but the user's
    // drag (adr/2026-07-positions-separate-file.md)
    let mut positions = use_signal({
        let root = root.clone();
        move || Positions::load(&root.join(".index/positions"))
    });
    // which screen is up; the logs remain the door the app opens on
    let mut screen = use_signal(|| Screen::Logs);
    // the one prose size the editor's textarea and every rendered fragment
    // share; the settings overlay's stepper is the one control that reaches
    // it (adr/2026-08-one-font-size-for-source-and-render.md,
    // adr/2026-08-settings-overlay.md)
    let mut font_size = use_signal(|| DEFAULT_SIZE);
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
    // the loops overlay's highlighted row, the link picker's idiom over a
    // list with no query field: reset to 0 whenever the overlay opens
    // (`toggle_loops`), so a stale rank from a shorter list never survives
    let mut loops_highlighted = use_signal(|| 0usize);
    // the status surface: every notice and the liveness fact — one owner,
    // one line, one glyph (adr/2026-08-status-surface-owns-notices.md)
    let mut status = use_signal(Status::default);
    // the notices overlay, the loops list's sibling: the history behind a
    // palette command, closed by the same Escape ladder
    let mut notices_open = use_signal(|| false);
    // the settings overlay, the notices overlay's sibling: session-only,
    // holding the theme toggle and the font-size stepper
    // (adr/2026-08-settings-overlay.md)
    let mut settings_open = use_signal(|| false);
    // the temporal panes' folds, session-only like the settings knobs
    // (adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md)
    let mut rail_folded = use_signal(|| false);
    let mut jump_folded = use_signal(|| false);
    // the visit log: what `select` and `show_sheet` were showing right
    // before they changed it, capped so the log stays bounded — the
    // switcher's empty-query list reads it
    // (adr/2026-08-note-history-back.md,
    // adr/2026-09-ctrl-o-is-the-one-note-switcher.md)
    let mut history = use_signal(Vec::<Visit>::new);
    // set around the one call that returns to a place already showing —
    // the template-editing Escape, which routes through `select` with the
    // selection already standing — so that return does not get pushed onto
    // the log as a new visit. The switcher's own landings are real visits
    // and push like any other
    // (adr/2026-09-ctrl-o-is-the-one-note-switcher.md).
    let mut restoring_history = use_signal(|| false);
    let mut selected = use_signal(|| (NoteType::Daily, time::day_id(today)));
    let mut month = use_signal(|| today.first_of_month());
    // the fragment cache is a memo store, not UI state: nothing should
    // re-render when it fills, so a plain hook value rather than a signal
    let fragments =
        use_hook(|| Rc::new(RefCell::new(FragmentCache::default())));
    // the body cache, its table-side sibling: per-note SVGs living until
    // the watcher invalidates them (adr/2026-08-body-cache-per-note-svg.md)
    let bodies = use_hook(|| Rc::new(RefCell::new(BodyCache::default())));
    // the compiles' repaint tick: the drain bumps it once per landed burst,
    // and the shell reading it below is what re-renders the fresh SVGs in —
    // the caches themselves stay plain memo stores
    let mut compiled = use_signal(|| 0u64);
    // the launch survey (adr/2026-08-startup-survey-async.md): the inline
    // adapter answers before the first paint — the synchronous launch of
    // old — while the threaded one mounts the shell empty and lands the
    // survey like a watcher batch, through the drain below
    use_hook({
        let root = root.clone();
        let feed = feed.clone();
        move || {
            if !feed.inline {
                (feed.submit)(compute::rescan(&root, false, today));
                return;
            }
            match compute::refresh(&root, &[watch::VaultChange::Rescan], today)
            {
                Ok((time_notes, open, table, links)) => {
                    notes.set(time_notes);
                    loops.set(open);
                    table_notes.set(table);
                    edges.set(links);
                }
                // no escalation inline: retrying the same rescan in the
                // same breath proves nothing; the watcher's next batch is
                // the retry
                Err(message) => {
                    let mut status = status.write();
                    status.report(Notice::watcher_failed(&message));
                    status.set_liveness(Liveness::Degraded);
                }
            }
        }
    });
    // absent in headless tests that don't inject a fake: mouse presses then
    // land at the block's end and the clipboard chords quietly decline
    let hit = try_consume_context::<HitProbe>();
    let line_probe = try_consume_context::<LineProbe>();
    let clipboard = try_consume_context::<Clipboard>();
    let clipboard_image = try_consume_context::<ClipboardImage>();
    let clipboard_write = try_consume_context::<ClipboardWrite>();
    let now = try_consume_context::<Now>();
    let launcher = try_consume_context::<Launcher>();
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
    // the app-level undo register: the before-images delete and arrange
    // leave behind (adr/2026-08-app-level-undo-register.md)
    let mut undo_register = use_signal(undo::Register::default);
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
    // what every query input may still be showing, one memory for the one
    // overlay open at a time (adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md)
    let mut shown = use_signal(Shown::default);

    // the active filter, and the Ctrl+F overlay that sets it — dims cards,
    // never drops them (adr/2026-08-filter-overlay-ctrl-f.md)
    let mut filter = use_signal(|| None::<table::Filter>);
    let mut filter_picker = use_signal(|| None::<FilterPicker>);
    let mut filter_query = use_signal(String::new);
    let mut filter_highlighted = use_signal(|| 0usize);
    // the Ctrl+O note switcher, the one picker that opens a note from
    // anywhere (adr/2026-09-ctrl-o-is-the-one-note-switcher.md)
    let mut switcher = use_signal(|| None::<Switcher>);
    let mut switcher_query = use_signal(String::new);
    let mut switcher_highlighted = use_signal(|| 0usize);
    // the Ctrl+Shift+F finder over the vault's text, the switcher's
    // twin (adr/2026-09-full-text-search-lives-in-the-index.md)
    let mut finder_open = use_signal(|| false);
    let mut finder_query = use_signal(String::new);
    let mut finder_highlighted = use_signal(|| 0usize);
    let mut finder_hits = use_signal(Vec::<crate::index::SearchHit>::new);
    // the edit-template picker, palette-summoned and logs-only
    // (adr/2026-08-template-editing-in-the-one-editor.md)
    let mut template_picker = use_signal(|| None::<TemplatePicker>);
    let mut template_query = use_signal(String::new);
    let mut template_highlighted = use_signal(|| 0usize);
    // the modal layer, one signal beside the editor's so both mounts share
    // the mode and it survives slides and activations
    // (adr/2026-08-escape-ladder-editor-wide-mode.md)
    let mut vim = use_signal(vim::Vim::default);
    // a composition is open — input state, true in every mode, unlike the
    // preview below which normal mode must never draw. The commit
    // keystroke can arrive unflagged (the spike's stray keydown,
    // adr/2026-08-hidden-ime-sink.md), and without this the grammar reads
    // it as a real key and resets the pending chord: d^ lost its d.
    let mut composing = use_signal(|| Composing::No);
    // the live IME composition ("^" mid–dead-key), previewed at the caret
    // and absent from the buffer until compositionend commits it
    // (adr/2026-08-hidden-ime-sink.md)
    let mut preview = use_signal(|| None::<String>);
    // the one-line / prompt (adr/2026-08-search-lands-through-place.md):
    // open-or-not plus its moving query, the overlay split as ever
    let mut search_prompt = use_signal(|| false);
    let mut search_query = use_signal(String::new);
    // the : prompt, the / prompt's twin: same widget, same region, other
    // sigil (adr/2026-08-ex-line-is-literal-and-global.md)
    let mut ex_prompt = use_signal(|| false);
    let mut ex_query = use_signal(String::new);
    // where the next caret mount should sit in the pane, and a nonce so a
    // bare zz — which moves the caret not at all — still remounts it. The
    // anchor is consumed by the mount that uses it and falls back to
    // Nearest, so an async fragment landing never moves the viewport on
    // its own (adr/2026-08-scroll-anchor-is-consumed-once.md)
    let mut scroll_anchor = use_signal(|| (vim::Anchor::Nearest, 0u32));
    // a drag in flight, and whether a hit probe is already out — plain
    // cells, like QuitFlush: only the mouse handlers read them
    let dragging = use_hook(|| Rc::new(std::cell::Cell::new(false)));
    let probing = use_hook(|| Rc::new(std::cell::Cell::new(false)));
    // the goal column a j/k run holds across its steps, pixel and logical
    // (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md)
    let goal = use_hook(|| Rc::new(std::cell::Cell::new(Goal::default())));

    // the editor's one exit for trouble: whatever any path deposited —
    // internal chains like activate → deactivate → flush included — is
    // drained here and reported, so the editor detects and status displays
    // (adr/2026-08-status-surface-owns-notices.md). The gate reads before
    // the drain writes, or the effect would subscribe to its own write and
    // spin forever.
    use_effect(move || {
        let pending = editor.read().trouble().is_some();
        if pending && let Some(trouble) = editor.write().take_trouble() {
            status.write().report(Notice::from_trouble(trouble));
        }
    });

    // the Ctrl+Q flush: reports whether the open note and the canvas
    // positions reached disk, so a failed save can hold the app open
    // instead of losing either
    let quit_flush = use_callback(move |()| {
        let note_saved = editor.write().flush();
        let placed_saved = match positions.peek().save() {
            Ok(()) => true,
            Err(error) => {
                status
                    .write()
                    .report(Notice::positions_failed(&error.to_string()));
                false
            }
        };
        note_saved && placed_saved
    });
    let register = use_context::<QuitFlush>();
    // once is enough: the Callback's identity is stable across re-renders,
    // only its captured closure is refreshed
    use_hook(move || register.0.borrow_mut().replace(quit_flush));

    // what the pre-launch seeding had to say, if `main` ran one: a vault
    // that would not take the default templates is reported once, here
    use_hook(move || {
        if let Some(SeedTrouble(Some(reason))) =
            try_consume_context::<SeedTrouble>()
        {
            status.write().report(Notice::seed_failed(&reason));
        }
    });

    // the vault watcher, if one was handed over: every batch it debounces
    // invalidates the stale bodies and rides the compute tier as a survey
    // job — the index work itself left this thread with C3
    // (adr/2026-08-compute-tier-worker-seam.md); the outcome lands on the
    // drain below (adr/2026-08-watcher-feeds-the-ui.md). Taken out of its
    // cell once; a second render finds `None` and starts nothing.
    // Liveness is established here and on the drain: Watching when the
    // receiver is taken, Unwatched when there is none to take, Degraded
    // when a survey fails.
    use_hook({
        let root = root.clone();
        let fragments = fragments.clone();
        let bodies = bodies.clone();
        let feed = feed.clone();
        move || {
            let Some(watched) = try_consume_context::<VaultFeed>() else {
                return;
            };
            if let Some(reason) = &watched.trouble {
                status.write().report(Notice::unwatched(reason));
                return;
            }
            let taken =
                watched.changes.lock().ok().and_then(|mut cell| cell.take());
            let Some(mut changes) = taken else { return };
            status.write().set_liveness(Liveness::Watching);
            spawn(async move {
                loop {
                    let Some(batch) = changes.recv().await else {
                        // The screen keeps the index it has, while the
                        // liveness glyph carries the stopped watcher's state.
                        status.write().set_liveness(Liveness::Unwatched);
                        break;
                    };
                    // the caches hear about every change first, so the
                    // repaint the survey triggers re-renders fresh pixels
                    // (adr/2026-08-body-cache-per-note-svg.md). A template
                    // edit — and a rescan, whose lost events could have
                    // been one — clears the fragments too: the template is
                    // the compile input their keys never carry
                    // (adr/2026-08-template-touch-clears-caches.md)
                    for change in &batch {
                        match change {
                            watch::VaultChange::Touched { path, .. }
                            | watch::VaultChange::Removed(path) => {
                                bodies.borrow_mut().invalidate(path);
                            }
                            watch::VaultChange::Template
                            | watch::VaultChange::Rescan => {
                                fragments.borrow_mut().clear();
                                bodies.borrow_mut().clear();
                            }
                        }
                    }
                    (feed.submit)(Job::Survey {
                        root: root.clone(),
                        batch,
                        escalated: false,
                        today,
                    });
                }
            });
        }
    });

    // the compute tier's drain: every outcome lands here — compiles into
    // their caches (one repaint tick per burst), surveys into the signals
    // the screens derive from. A failed survey degrades the liveness and
    // escalates to one bounded rescan — the watcher's own doctrine, applied
    // around it (adr/2026-08-failed-batch-escalates-to-rescan.md); the
    // escalation's success resolves the degradation like any clean batch.
    // Taken out of its cell once, like the watcher's receiver.
    use_hook({
        let root = root.clone();
        let feed = feed.clone();
        let fragments = fragments.clone();
        let bodies = bodies.clone();
        move || {
            let taken =
                feed.outcomes.lock().ok().and_then(|mut cell| cell.take());
            let Some(mut outcomes) = taken else { return };
            spawn(async move {
                // absorb outcomes in bursts before repainting once: a
                // theme toggle or a bodies zoom lands dozens together
                let mut burst = Vec::new();
                loop {
                    burst.clear();
                    if outcomes.recv_many(&mut burst, 64).await == 0 {
                        break;
                    }
                    let mut landed = false;
                    for outcome in burst.drain(..) {
                        match outcome {
                            Outcome::Fragment { key, epoch, result } => {
                                fragments
                                    .borrow_mut()
                                    .absorb(key, epoch, result);
                                landed = true;
                            }
                            Outcome::Body {
                                note,
                                theme,
                                epoch,
                                result,
                            } => {
                                bodies
                                    .borrow_mut()
                                    .absorb(note, theme, epoch, result);
                                landed = true;
                            }
                            Outcome::Export { result, .. } => {
                                let mut status = status.write();
                                match result {
                                    Ok(pdf) => {
                                        status.report(Notice::exported(&pdf))
                                    }
                                    Err(detail) => status.report(
                                        Notice::export_failed(&detail),
                                    ),
                                }
                            }
                            Outcome::Survey {
                                result: Ok((time_notes, open, table, links)),
                                ..
                            } => {
                                notes.set(time_notes);
                                loops.set(open);
                                table_notes.set(table);
                                edges.set(links);
                                // reaching here is the degradation's
                                // resolution, whether by clean batch or by
                                // the escalated rescan
                                if status.peek().liveness()
                                    == Liveness::Degraded
                                {
                                    let mut status = status.write();
                                    status.resolve(Source::Watcher);
                                    status.set_liveness(Liveness::Watching);
                                }
                            }
                            Outcome::Survey {
                                result: Err(message),
                                escalated,
                            } => {
                                {
                                    let mut status = status.write();
                                    status.report(Notice::watcher_failed(
                                        &message,
                                    ));
                                    status.set_liveness(Liveness::Degraded);
                                }
                                // updates were lost, so nothing derived
                                // can be trusted — but a rescan that
                                // itself failed escalates no further
                                if !escalated {
                                    bodies.borrow_mut().clear();
                                    (feed.submit)(compute::rescan(
                                        &root, true, today,
                                    ));
                                }
                            }
                        }
                    }
                    if landed {
                        *compiled.write() += 1;
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
            match editor.peek().save() {
                // gated like every status write from a ticking resource:
                // the same failure re-reported would repaint for nothing.
                // A refused disk and a refused clobber both surface here —
                // the conflict's notice summons the palette pair
                // (adr/2026-08-external-edit-conflict-commands.md)
                Some(trouble) => {
                    let notice = Notice::from_trouble(trouble);
                    if !status.peek().showing(&notice) {
                        status.write().report(notice);
                    }
                }
                // the save that lands resolves its own failure — no
                // gesture, the condition simply ceased
                // (adr/2026-08-status-surface-owns-notices.md)
                None => {
                    if status.peek().has(Source::Save) {
                        status.write().resolve(Source::Save);
                    }
                }
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
            match positions.peek().save() {
                // gated like the autosave's: the same failure re-reported
                // would repaint for nothing
                Err(error) => {
                    let notice = Notice::positions_failed(&error.to_string());
                    if !status.peek().showing(&notice) {
                        status.write().report(notice);
                    }
                }
                Ok(()) => {
                    if status.peek().has(Source::Positions) {
                        status.write().resolve(Source::Positions);
                    }
                }
            }
        }
    });

    let select = use_callback({
        let root = root.clone();
        let fragments = fragments.clone();
        move |target: Selection| {
            if !editor.write().flush() {
                return;
            }
            // the visit log: what stood here a moment ago, before
            // this selection replaces it — unless this call is itself a
            // landing on a popped visit, in which case pushing would put
            // it straight back (adr/2026-08-note-history-back.md)
            if !*restoring_history.peek() {
                let visit = match sheet.peek().clone() {
                    Some(own) => Visit::Sheet(own),
                    None => Visit::Logs(selected.peek().clone()),
                };
                history.with_mut(|stack| push_visit(stack, visit));
            }
            // a sheet open for another note must not survive the switch —
            // its raised card, tether and backlinks belong to the note the
            // editor is about to leave, and the palette's sheet-only
            // commands must stop reading `own` from a note the editor no
            // longer holds (adr/2026-08-sheet-reuses-the-one-editor.md)
            if sheet.peek().is_some() {
                picker.set(None);
                sheet.set(None);
            }
            let anchor = logs::selection_date(&target.0, &target.1);
            // every selectable id comes from our own formatters, so the today
            // fallback guards the type system, not a reachable path
            month.set(anchor.unwrap_or(today).first_of_month());
            let exists = notes
                .peek()
                .iter()
                .any(|(existing, _)| existing == &target.1);
            editor.set(open_selected(&root, exists, &target.1));
            vim.write().note_opened();
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
            // the visit log, the same push `select` makes —
            // unconditional here: the one suppressed return (the
            // template-editing Escape) routes through `select`, never
            // through a sheet (adr/2026-09-ctrl-o-is-the-one-note-switcher.md)
            let visit = match sheet.peek().clone() {
                Some(own) => Visit::Sheet(own),
                None => Visit::Logs(selected.peek().clone()),
            };
            history.with_mut(|stack| push_visit(stack, visit));
            // the sheet, tether and raised card are titles-zoom constructs:
            // opening one zooms out first, one legible gesture
            // (adr/2026-08-body-zoom-scale-and-metrics.md)
            zoom_to.call(table::Zoom::Titles);
            picker.set(None);
            editor.set(opened);
            vim.write().note_opened();
            fragments.borrow_mut().sweep();
            screen.set(Screen::Table);
            sheet.set(Some(id));
        }
    });
    let open_sheet = use_callback({
        let root = root.clone();
        move |id: String| {
            // already showing this sheet AND actually holding its note —
            // the one repeat worth swallowing. A failed lookup's bare
            // sheet (closed editor) stays retryable: the next click on
            // the same card re-runs the lookup instead of standing on the
            // error, which is the natural retry once the index heals
            // (adr/2026-09-index-notices-resolve-on-a-good-lookup.md)
            if sheet.peek().as_deref() == Some(id.as_str())
                && editor.peek().note().is_some()
            {
                return;
            }
            // a lookup that fails still opens the sheet: the closed editor
            // keeps the pane bare and the notice line — rendered inside the
            // sheet — puts the message where the user is looking
            let (opened, found) = match open_sheet_note(&root, &id) {
                Ok(opened) => (opened, true),
                Err(message) => {
                    status.write().report(Notice::index(message));
                    (Editor::closed(), false)
                }
            };
            show_sheet.call((id.clone(), opened));
            // resolved only once `show_sheet` actually landed on this id: a
            // refused flush (an unsaved sheet's own save failing) leaves
            // the prior sheet standing, and a lookup that merely succeeded
            // is not proof the navigation the user asked for happened
            if found
                && sheet.peek().as_deref() == Some(id.as_str())
                && status.peek().has(Source::Index)
            {
                status.write().resolve(Source::Index);
            }
        }
    });
    // the one id -> destination rule: a time note lands on the logs,
    // everything the table already knows about opens through the sheet,
    // and an id nothing in the vault knows — a dangling link's target —
    // stays inert, the same gate the links footer applies
    // (adr/2026-08-permanent-links-open-sheets.md: "dangling links stay
    // inert"). Every entry point that names a note by id — gf, Ctrl+Enter,
    // the palette's follow link — shares this one rule, so none of them
    // needs its own per-category match. The loops list does NOT route
    // through here: a loop line opens its note by path
    // (`open_loop`, `adr/2026-09-loop-lines-open-their-notes.md`), because
    // some of what it names — a note with no `#meta` at all — has no id
    // row for this gate to find in the first place.
    let open_id = use_callback(move |id: String| {
        if let Some(scale) = links::scale_of(&id, &notes.peek()) {
            // a time link followed from a sheet lands on the logs.
            // `select` runs first: it records the sheet on the visit log
            // and closes it itself — closing here first made the log
            // record the logs selection the sheet stood over instead of
            // the sheet. The screen only switches once the sheet really
            // closed: a refused flush keeps the sheet, so it must keep
            // its screen too
            let from_sheet = sheet.peek().is_some();
            select.call((scale, id));
            if from_sheet && sheet.peek().is_none() {
                screen.set(Screen::Logs);
            }
        } else if table_notes.peek().iter().any(|note| note.id == id) {
            open_sheet.call(id);
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
            // an error sheet (a failed lookup's own closed editor) is the
            // one thing a "sheet: no note has the id" notice can outlive
            // with no later index read to prove it stale: leaving it ends
            // the notice's own cause just as directly as a fresh success
            // would (adr/2026-09-index-notices-resolve-on-a-good-lookup.md)
            if sheet.peek().is_some()
                && editor.peek().note().is_none()
                && status.peek().has(Source::Index)
            {
                status.write().resolve(Source::Index);
            }
            picker.set(None);
            sheet.set(None);
            let id = selected.peek().1.clone();
            let exists =
                notes.peek().iter().any(|(existing, _)| existing == &id);
            editor.set(open_selected(&root, exists, &id));
            vim.write().note_opened();
            fragments.borrow_mut().sweep();
        }
    });

    // the sheet's delete (adr/2026-08-delete-note-palette-only-from-sheet.md):
    // unconfirmed, no trash. Deliberately not `close_sheet` — its flush
    // would rewrite the just-deleted file from the buffer. The landing half
    // is close_sheet's minus the flush: card and position go optimistically,
    // and the app indexes its own removal in the same tick rather than
    // waiting on the watcher (adr/2026-09-the-app-indexes-its-own-writes.md)
    // — the dangling links the deletion causes surface in the loops list
    // as designed.
    let delete_note = use_callback({
        let root = root.clone();
        let fragments = fragments.clone();
        let feed = feed.clone();
        let bodies = bodies.clone();
        move |()| {
            let Some(own) = sheet.peek().clone() else {
                return;
            };
            // an error sheet holds a closed editor: nothing on disk to
            // remove, the sheet still deserves to close
            let file =
                editor.peek().note().map(|(file, _)| file.to_path_buf());
            // the before-image, taken while the file still answers: its
            // text and its card's coordinates are everything the undo
            // restores (adr/2026-08-app-level-undo-register.md)
            let intent = file.as_ref().and_then(|file| {
                std::fs::read_to_string(file).ok().map(|text| {
                    undo::Intent::Delete {
                        path: file.clone(),
                        id: own.clone(),
                        text,
                        position: positions.peek().get(&own),
                    }
                })
            });
            let relative =
                file.as_ref().map(|file| vault_relative(&root, file));
            if let Some(file) = file
                && let Err(error) = std::fs::remove_file(&file)
            {
                status
                    .write()
                    .report(Notice::delete_failed(&error.to_string()));
                return;
            }
            if let Some(intent) = intent {
                undo_register.write().push(intent);
            }
            // the app indexes its own delete in the same tick rather than
            // waiting on the watcher (adr/2026-09-the-app-indexes-its-own-writes.md)
            if let Some(relative) = relative {
                bodies.borrow_mut().invalidate(&relative);
                (feed.submit)(compute::removed(&root, relative, today));
            }
            sheet.set(None);
            positions.write().remove(&own);
            table_notes.with_mut(|list| list.retain(|note| note.id != own));
            let id = selected.peek().1.clone();
            let exists =
                notes.peek().iter().any(|(existing, _)| existing == &id);
            editor.set(open_selected(&root, exists, &id));
            vim.write().note_opened();
            fragments.borrow_mut().sweep();
        }
    });

    // the create overlay's opening half — summon_palette's twin
    // (adr/2026-08-ctrl-n-two-step-create-overlay.md)
    let open_creator = use_callback(move |()| {
        creator_query.set(String::new());
        shown.set(Shown::opened(""));
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
        let feed = feed.clone();
        let bodies = bodies.clone();
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
                    let relative = vault_relative(&root, &path);
                    // optimistic in-memory push, and the app indexes its
                    // own write in the same tick rather than waiting on the
                    // watcher (adr/2026-09-the-app-indexes-its-own-writes.md)
                    table_notes.with_mut(|list| {
                        list.push(TableNote {
                            id: id.clone(),
                            path: relative.clone(),
                            kind: NoteCategory::Permanent,
                            note_type: Some(picked),
                            title: Some(title.clone()),
                            created: Some(created),
                            tags: Vec::new(),
                        });
                    });
                    bodies.borrow_mut().invalidate(&relative);
                    (feed.submit)(compute::touched(
                        &root,
                        NoteCategory::Permanent,
                        relative,
                        today,
                    ));
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
                if status.peek().has(Source::Index) {
                    status.write().resolve(Source::Index);
                }
                filter_query.set(String::new());
                shown.set(Shown::opened(""));
                filter_highlighted.set(0);
                filter_picker.set(Some(FilterPicker {
                    entries: table::filter_entries(&tags),
                }));
            }
            Err(msg) => status.write().report(Notice::index(msg)),
        }
    });
    // closing hands focus back through the focus effect, like every overlay
    let close_filter = use_callback(move |()| filter_picker.set(None));
    let apply_filter = use_callback(move |chosen: Option<table::Filter>| {
        filter.set(chosen);
        close_filter.call(());
    });

    // the template picker's opening half: the directory listed at the
    // moment it opens — templates are never in the index, so the
    // filesystem is the authority
    // (adr/2026-08-template-editing-in-the-one-editor.md)
    let open_templates = use_callback({
        let root = root.clone();
        move |()| match template_names(&root) {
            Ok(entries) => {
                if status.peek().has(Source::Index) {
                    status.write().resolve(Source::Index);
                }
                template_query.set(String::new());
                shown.set(Shown::opened(""));
                template_highlighted.set(0);
                template_picker.set(Some(TemplatePicker { entries }));
            }
            Err(msg) => status.write().report(Notice::index(msg)),
        }
    });
    let close_templates = use_callback(move |()| template_picker.set(None));
    // the landing half, the sheet's flush discipline: the buffer reaches
    // disk before it is replaced, and a failed save keeps the current note
    // open with its error rather than dropping the text
    let edit_template = use_callback({
        let root = root.clone();
        let fragments = fragments.clone();
        move |name: String| {
            if !editor.write().flush() {
                return;
            }
            // reached from the table, the pick carries the screen with it:
            // the template still opens in the logs' centre pane, the one
            // full-page surface the shared editor has
            // (adr/2026-09-edit-template-reaches-the-logs-from-the-table.md).
            // A sheet open over the table holds that same editor, so it
            // gets `select`'s bookkeeping first — the sheet onto the switcher's
            // log (Escape out of a template lands on the logs selection,
            // so the visit log is the only way back to the sheet), then
            // card and picker closed — and only then does the screen
            // change, the order the table's own Ctrl+D follows
            // (`open_daily` before `go_logs`). Not `go_logs`: its
            // `close_sheet` would put the logs' selected note back into
            // the editor the template is about to take. Cloned out first,
            // `select`'s own idiom: the peek guard must drop before the
            // body writes the signal back.
            let sheeted = sheet.peek().clone();
            if let Some(own) = sheeted {
                history.with_mut(|stack| push_visit(stack, Visit::Sheet(own)));
                picker.set(None);
                sheet.set(None);
            }
            close_templates.call(());
            editor.set(Editor::open(
                root.join("templates").join(format!("{name}.typ")),
            ));
            vim.write().note_opened();
            fragments.borrow_mut().sweep();
            // a no-op on the logs, where the pane already stands
            screen.set(Screen::Logs);
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
            // the before-image: every card about to move, at its prior
            // coordinates — `None` for an auto-placed card, whose reverse
            // is unpinning; an arrange that moved nothing leaves nothing
            // to take back (adr/2026-08-app-level-undo-register.md)
            let before = (!laid.is_empty()).then(|| undo::Intent::Arrange {
                prior: laid
                    .iter()
                    .map(|(id, _)| (id.clone(), positions.peek().get(id)))
                    .collect(),
            });
            before
                .into_iter()
                .for_each(|intent| undo_register.write().push(intent));
            // one write: one repaint, one debounce restart
            positions.with_mut(|store| {
                for (id, (x, y)) in &laid {
                    store.set(id, *x, *y);
                }
            });
        }
    });

    // the last destruction, taken back
    // (adr/2026-08-app-level-undo-register.md): a deleted note returns
    // exactly as it left — `create_new`, never a clobber of a path that
    // holds a living file again — and an arrange returns every card it
    // moved. The restored note is a fifth write seam indexing itself in
    // the same tick, the delete's own landing half leans on. The palette hides
    // the command when the register is empty, so the pop is combinator-fed
    // rather than guarded.
    let undo_last = use_callback({
        let root = root.clone();
        let feed = feed.clone();
        let bodies = bodies.clone();
        move |()| {
            let intent = undo_register.write().pop();
            intent.into_iter().for_each(|intent| match intent {
                undo::Intent::Delete {
                    path,
                    id,
                    text,
                    position,
                } => match crate::persist::create_new(&path, &text) {
                    Ok(()) => {
                        if let Some((x, y)) = position {
                            positions.write().set(&id, x, y);
                        }
                        // the app indexes its own write in the same tick
                        // rather than waiting on the watcher — the same seam
                        // create_note and delete_note already use
                        // (adr/2026-09-the-app-indexes-its-own-writes.md)
                        let relative = vault_relative(&root, &path);
                        let category = dir_category(&relative);
                        bodies.borrow_mut().invalidate(&relative);
                        (feed.submit)(compute::touched(
                            &root, category, relative, today,
                        ));
                    }
                    Err(error) => {
                        status
                            .write()
                            .report(Notice::undo_failed(&error.to_string()));
                    }
                },
                undo::Intent::Arrange { prior } => {
                    positions.with_mut(|store| {
                        for (id, at) in &prior {
                            match at {
                                Some((x, y)) => store.set(id, *x, *y),
                                None => store.remove(id),
                            }
                        }
                    });
                }
            });
        }
    });

    // the small movements, lifted so chord, button, wheel and palette all
    // run one path (adr/2026-08-palette-birth-command-list.md)
    let page = use_callback(move |forward: bool| {
        month.set(logs::page_month(month(), forward));
    });
    let toggle_loops = use_callback(move |()| {
        let opening = !loops_open();
        loops_open.set(opening);
        if opening {
            loops_highlighted.set(0);
        }
    });
    // a click or Enter on a loops-list row: close the overlay, then open
    // the note by its own path — never through `open_id`'s id lookup. A
    // note that owes debt because its own `#meta` is missing or broken
    // has no id row `path_for_id` could resolve, so this is not a fifth
    // branch of the id -> destination rule, it is the one place that
    // still has the path and uses it instead of round-tripping through an
    // id the index may not have. The destination still follows the
    // category rule every other entry point applies: a time note lands on
    // the logs, where its rail, calendar and crumbs are — a sheet over
    // the table would float it with no card behind it. Only a time file
    // whose stem no scale can parse falls back to the sheet, the one
    // surface that can show a file the logs cannot place
    // (`adr/2026-09-loop-lines-open-their-notes.md`)
    let open_loop = use_callback({
        let root = root.clone();
        move |path: PathBuf| {
            loops_open.set(false);
            let id = crate::domain::stem_of(&path);
            if dir_category(&path) == NoteCategory::Time
                && let Some(scale) = logs::scale_of_id(&id)
            {
                select.call((scale, id.clone()));
                // the screen only switches once the selection really
                // landed: a refused flush keeps the editor — and any
                // sheet — where they were, so they keep their screen too
                // (`open_id`'s own guard)
                if sheet.peek().is_none() && selected.peek().1 == id {
                    screen.set(Screen::Logs);
                }
                return;
            }
            let opened = Editor::open(root.join(&path));
            show_sheet.call((id, opened));
        }
    });
    // the notices overlay's toggle, the loops list's twin
    let toggle_notices =
        use_callback(move |()| notices_open.set(!notices_open()));
    // the settings overlay's open: it only ever closes through Escape or
    // its own onkeydown, so unlike the toggles above it has one direction
    // (adr/2026-08-settings-overlay.md)
    let open_settings = use_callback(move |()| settings_open.set(true));
    // one toggle for both chords, both palette rows and the sink's arm
    let fold_pane = use_callback(move |fold: keymap::Fold| match fold {
        keymap::Fold::Rail => rail_folded.set(!rail_folded()),
        keymap::Fold::Jump => jump_folded.set(!jump_folded()),
    });
    let open_daily = use_callback(move |()| {
        select.call((NoteType::Daily, time::day_id(today)))
    });
    let open_weekly = use_callback(move |()| {
        select.call((NoteType::Weekly, time::week_id(today)))
    });
    let open_season = use_callback(move |()| {
        select.call((NoteType::Seasonal, time::season_id(today)))
    });
    // the Enter arm's creation half, lifted so the palette's "open next"
    // shares it: create from template and push onto the note list, only —
    // opening is `select`'s job, the one place that flushes the editor
    // being left and clears a sheet the switch would otherwise swap out
    // from under (adr/2026-08-time-navigation-commands.md). Returns
    // whether creation landed, so a caller that only opens on success
    // never moves the selection onto a period with no note.
    let create_time_note = use_callback({
        let root = root.clone();
        let feed = feed.clone();
        let bodies = bodies.clone();
        move |(scale, id): (NoteType, String)| -> bool {
            let created = logs::selection_date(&scale, &id).unwrap_or(today);
            match crate::template::create(
                &root,
                &NoteCategory::Time,
                &scale,
                &id,
                &created.to_string(),
                "",
            ) {
                Ok(path) => {
                    let relative = vault_relative(&root, &path);
                    bodies.borrow_mut().invalidate(&relative);
                    (feed.submit)(compute::touched(
                        &root,
                        NoteCategory::Time,
                        relative,
                        today,
                    ));
                    notes.with_mut(|list| list.push((id, scale)));
                    true
                }
                Err(err) => {
                    status
                        .write()
                        .report(Notice::create_failed(&format!("{err:?}")));
                    false
                }
            }
        }
    });
    // the six relative commands' shared body: the anchor is the open
    // note's own date, so the command is total even from a selection at
    // another scale (adr/2026-08-time-navigation-commands.md). Forward
    // creates from the template when the target is missing; backward only
    // ever resolves to what already exists and never writes.
    let step_time = use_callback(move |(scale, forward): (NoteType, bool)| {
        let anchor =
            logs::selection_date(&selected.peek().0, &selected.peek().1)
                .unwrap_or(today);
        if forward {
            logs::next_period(&scale, anchor)
                .into_iter()
                .for_each(|id| {
                    let exists = notes
                        .peek()
                        .iter()
                        .any(|(existing, _)| existing == &id);
                    // a refused creation leaves the selection where it
                    // stood — `select` never runs over a period with no
                    // note behind it (adr/2026-08-time-navigation-commands.md)
                    let ready = exists
                        || create_time_note.call((scale.clone(), id.clone()));
                    if ready {
                        select.call((scale.clone(), id));
                    }
                });
        } else {
            // no note before the first one: the selection holds
            // (adr/2026-08-time-navigation-commands.md)
            logs::previous_existing(&notes.peek(), &scale, anchor)
                .into_iter()
                .for_each(|id| select.call((scale.clone(), id)));
        }
    });
    // the screen switch, one seam for icon, chord and palette
    // (adr/2026-08-screen-switch-gesture.md). Leaving the logs closes the
    // active block and the picker the way Escape would: their textarea and
    // input are about to unmount, and a hidden overlay waiting behind a
    // screen would reopen unasked on the way back.
    let go_table = use_callback(move |()| {
        // the block stays active behind the screen switch — the cursor
        // belongs to the note, and returning to the logs finds it again
        // (adr/2026-08-cursor-always-in-the-note.md); only the pickers
        // close, their inputs being about to unmount
        picker.set(None);
        template_picker.set(None);
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
        // the template picker now stands over this screen too, so it
        // leaves the way it leaves the logs — go_table's mirror
        // (adr/2026-09-edit-template-reaches-the-logs-from-the-table.md)
        template_picker.set(None);
        screen.set(Screen::Logs);
    });
    // Ctrl+O, the one note switcher, opening over every screen
    // (adr/2026-09-ctrl-o-is-the-one-note-switcher.md). Its two halves are
    // frozen at open, the `Picker` idiom: where you have been — the visit
    // log's distinct notes, newest first — and everywhere you could go,
    // every note the index knows. Both leave out the note currently
    // showing. Unlike the picker it replaces, an empty log is not a
    // no-op: the switcher opens so the query can be typed.
    let open_switcher = use_callback({
        let root = root.clone();
        move |()| {
            let own = match sheet.peek().clone() {
                Some(own) => own,
                None => selected.peek().1.clone(),
            };
            // an index that will not open costs the typed half only: the
            // visit log is app state, and where you have been stays
            // reachable while the notice says why the rest is not
            let entries: Vec<links::Completion> = match completions(&root) {
                Ok(entries) => {
                    if status.peek().has(Source::Index) {
                        status.write().resolve(Source::Index);
                    }
                    entries
                        .into_iter()
                        .filter(|entry| entry.id != own)
                        .collect()
                }
                Err(msg) => {
                    status.write().report(Notice::index(msg));
                    Vec::new()
                }
            };
            let recent = recent_notes(&history.peek(), &own, &entries);
            switcher_query.set(String::new());
            shown.set(Shown::opened(""));
            switcher_highlighted.set(0);
            switcher.set(Some(Switcher { recent, entries }));
        }
    });
    let close_switcher = use_callback(move |()| switcher.set(None));
    // the finder: opens empty, searches on every keystroke — the index is
    // local and the vault small, so the answer lands inside the keystroke
    // — and lands a hit the way a loop line lands, by path
    let open_finder = use_callback(move |()| {
        finder_query.set(String::new());
        shown.set(Shown::opened(""));
        finder_highlighted.set(0);
        finder_hits.set(Vec::new());
        finder_open.set(true);
    });
    let close_finder = use_callback(move |()| finder_open.set(false));
    let finder_typed = use_callback({
        let root = root.clone();
        move |query: String| {
            finder_highlighted.set(0);
            match search_hits(&root, &query) {
                Ok(hits) => {
                    status.write().resolve(Source::Index);
                    finder_hits.set(hits);
                }
                Err(msg) => {
                    status.write().report(Notice::index(msg));
                    finder_hits.set(Vec::new());
                }
            }
            finder_query.set(query);
        }
    });
    // the landing, following the category rule a loop line follows
    // (adr/2026-09-loop-lines-open-their-notes.md): a time note lands on
    // the logs, where its rail, calendar and crumbs are, and everything
    // else opens the sheet the table hosts. The verdict is read off what
    // the app already holds rather than a second index read — every row
    // came from `completions`, and `table_notes` is exactly the notes
    // outside `time/` that have an id, so "no card claims this id" *is*
    // the leading directory's answer. A time file whose stem no scale can
    // parse falls through to the sheet, the one surface that shows any
    // file, exactly as `open_loop` lets it.
    //
    // Landing is a real visit, pushed like any other by `select` and
    // `show_sheet` — no pop, no push suppression, so the switcher can
    // bounce. The picker closes only once the landing really happened:
    // both seams flush the buffer they are leaving first, and a refused
    // flush must leave the user looking at the list, not at a note that
    // never moved (`open_id`'s own guard).
    let switch_to = use_callback(move |id: String| {
        let carded = table_notes.peek().iter().any(|note| note.id == id);
        if !carded && let Some(scale) = logs::scale_of_id(&id) {
            select.call((scale, id.clone()));
            if sheet.peek().is_none() && selected.peek().1 == id {
                screen.set(Screen::Logs);
                switcher.set(None);
            }
            return;
        }
        open_sheet.call(id.clone());
        if sheet.peek().as_deref() == Some(id.as_str()) {
            switcher.set(None);
        }
    });

    // Where the logs pane is, so focus can be put back on it. A keydown
    // only bubbles up from whatever has focus, and the window's chord
    // (Ctrl+Q) is handled on the app root — so when the active block's
    // textarea unmounts, the webview drops focus on `<body>`, which is
    // *above* the app and outside every handler it has, and the chord
    // goes dead until something inside is clicked. A plain cell, like
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
            // mirrors the loops overlay's own mount guard (line ~3239):
            // nothing renders at zero loops, so an empty vault must not
            // starve the pane of focus
            let listing = loops_open() && !loops.read().is_empty();
            let overlaid = palette.read().is_some()
                || creator.read().is_some()
                || picker.read().is_some()
                || filter_picker.read().is_some()
                || switcher.read().is_some()
                || template_picker.read().is_some()
                || search_prompt()
                || ex_prompt()
                || finder_open()
                || listing
                // absent from this list, closing either overlay left the
                // focus stranded on <body>: the effect never re-ran, the
                // sink never got the focus back, and buffered keystrokes
                // replayed on the next click
                || settings_open()
                || notices_open();
            // the sheet and the screen, the two elements the effect used to
            // leave unread (adr/2026-09-sheet-and-screen-join-the-focus-effect.md).
            // `blocks_view` — and the sink inside it — only mounts on the
            // logs screen or behind an open sheet; the table screen with no
            // sheet renders neither. Closing a sheet can still leave the one
            // editor `active` (the reactivated note wakes with its last
            // block, adr/2026-08-cursor-always-in-the-note.md), so `editing`
            // alone would aim at a sink that just unmounted with the sheet —
            // a stale handle nothing answers. Reading `sheet` and `screen`
            // every run, and gating the sink on an actual host for it,
            // means every screen switch and every sheet open-or-close
            // re-picks the right target instead of stranding focus on
            // `<body>`.
            let sheeted = sheet.read().is_some();
            let hosted = screen() == Screen::Logs || sheeted;
            let target = if editing && !listing && hosted {
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

    // A bare key that reaches the sink or a pane while an overlay is up
    // was typed at the overlay before its async focus grab landed
    // (adr/2026-09-overlay-keys-relay-before-focus-lands.md). The grammar
    // must never see it — "c" is an operator there, not a letter — so it
    // goes into the open overlay's query instead, or is dropped when the
    // overlay has none. Escape and the chords still pass: the ladders and
    // the chord arms below own those whether or not an overlay is up.
    let relay = use_callback(move |(key, modifiers): (Key, Modifiers)| {
        if key == Key::Escape
            || modifiers.intersects(
                Modifiers::CONTROL | Modifiers::ALT | Modifiers::META,
            )
        {
            return false;
        }
        let listing = loops_open() && !loops.read().is_empty();
        let open_query = [
            (palette.read().is_some(), palette_query),
            (creator.read().is_some(), creator_query),
            (picker.read().is_some(), query),
            (filter_picker.read().is_some(), filter_query),
            (switcher.read().is_some(), switcher_query),
            (template_picker.read().is_some(), template_query),
            (search_prompt(), search_query),
            (ex_prompt(), ex_query),
            (finder_open(), finder_query),
        ]
        .into_iter()
        .find_map(|(open, query)| open.then_some(query));
        let Some(mut open_query) = open_query else {
            return listing || settings_open() || notices_open();
        };
        match key {
            Key::Character(character) => {
                open_query.write().push_str(&character);
            }
            Key::Backspace => {
                open_query.write().pop();
            }
            _ => return true,
        }
        // the field learns of the letter by a patch it may not have seen
        // when it next speaks
        let written = open_query.peek().clone();
        shown.write().wrote(&written);
        true
    });

    // an `input` event names the field's whole value, and the field may be
    // behind a write the relay or a step change just made: the value is
    // read as a delta against what the field showed, never taken whole
    // (adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md)
    let typed =
        use_callback(move |(query, reported): (Signal<String>, String)| {
            shown.write().typed(&query.peek(), &reported)
        });

    // one follow path for Ctrl+Enter, Ctrl+click and the palette: the
    // caret is app state now, so everyone reads the same one — no probe,
    // no frozen offsets (adr/2026-08-ctrl-enter-opens-time-links.md,
    // adr/2026-08-caret-on-editor-note-bytes.md)
    let follow_at = use_callback({
        let root = root.clone();
        let launcher = launcher.clone();
        move |()| {
            let target = {
                let editor = editor.peek();
                let (_, head) = editor.caret_in_block();
                editor
                    .active_source()
                    .and_then(|slice| links::link_at(slice, head))
            };
            match target {
                Some(links::LinkTarget::Note(id)) => open_id.call(id),
                // a resource leaves the app: the launcher gets the
                // destination resolved against the vault, and the one
                // thing it can say back is that it would not start
                // (adr/2026-09-link-is-for-resources.md)
                Some(links::LinkTarget::Resource(destination)) => {
                    let Some(launcher) = &launcher else { return };
                    let resolved =
                        links::resolve_resource(&root, &destination);
                    match (launcher.0)(&resolved) {
                        Ok(()) => status.write().resolve(Source::Launcher),
                        Err(detail) => {
                            status.write().report(Notice::open_failed(&detail))
                        }
                    }
                }
                None => {}
            }
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
        let feed = feed.clone();
        let bodies = bodies.clone();
        move |()| {
            let Some(clipboard) = clipboard.clone() else {
                return;
            };
            let Some(now) = now.clone() else { return };
            let root = root.clone();
            let feed = feed.clone();
            let bodies = bodies.clone();
            spawn(async move {
                let Some(pasted) =
                    clipboard_answer((clipboard.0)().await, status)
                else {
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
                        // the app indexes its own write in the same tick
                        // rather than waiting on the watcher
                        // (adr/2026-09-the-app-indexes-its-own-writes.md)
                        let relative = vault_relative(&root, &path);
                        bodies.borrow_mut().invalidate(&relative);
                        (feed.submit)(compute::touched(
                            &root,
                            NoteCategory::Capture,
                            relative,
                            today,
                        ));
                        Notice::captured(&crate::domain::stem_of(&path))
                    }
                    Err(err) => Notice::capture_failed(&format!("{err:?}")),
                };
                status.write().report(notice);
            });
        }
    });

    // the picker's opening half; the splice point is the caret itself,
    // which the overlay cannot move (adr/2026-08-ctrl-l-link-picker.md)
    let open_picker = use_callback({
        let root = root.clone();
        move |()| match completions(&root) {
            Ok(entries) => {
                if status.peek().has(Source::Index) {
                    status.write().resolve(Source::Index);
                }
                query.set(String::new());
                shown.set(Shown::opened(""));
                highlighted.set(0);
                picker.set(Some(Picker { entries }));
            }
            Err(msg) => status.write().report(Notice::index(msg)),
        }
    });

    // closing an overlay is just closing it: the focus effect above sees
    // the signal flip and hands the focus back to the sink or the pane,
    // and the caret never moved (adr/2026-08-caret-on-editor-note-bytes.md)
    let close_picker = use_callback(move |()| picker.set(None));
    let close_palette = use_callback(move |()| palette.set(None));

    // "export pdf": the open note, flushed first so the pdf says what the
    // screen says, compiled and written beside itself on the compute tier;
    // the landing is a notice either way
    // (adr/2026-09-export-writes-the-pdf-beside-the-note.md)
    let export_pdf = use_callback({
        let root = root.clone();
        let feed = feed.clone();
        move |()| {
            // the palette lists the command only over an open note; a
            // flush the disk refuses already stands on the line as the
            // save's own critical, so nothing is exported and nothing more
            // is said
            let note = editor
                .peek()
                .note()
                .map(|(path, _)| vault_relative(&root, path));
            if let Some(note) = note
                && editor.write().flush()
            {
                (feed.submit)(compute::export(&root, note));
            }
        }
    });

    // the palette's opening half, shared by both screens' Ctrl+P
    // (adr/2026-08-command-palette-overlay-shape.md)
    let summon_palette = use_callback(move |()| {
        palette_query.set(String::new());
        shown.set(Shown::opened(""));
        palette_highlighted.set(0);
        palette.set(Some(Palette {
            block_active: editor.peek().active().is_some(),
            note_open: editor.peek().note().is_some(),
            on_table: *screen.peek() == Screen::Table,
            sheet_open: sheet.peek().is_some(),
            at_bodies: *zoom.peek() == table::Zoom::Bodies,
            conflict: status.peek().has(Source::Conflict),
            undoable: undo_register.peek().label().is_some(),
        }));
    });

    // the conflict's fork (adr/2026-08-external-edit-conflict-commands.md):
    // the guard refused to clobber an edit made outside the app, and the
    // app cannot merge — each command picks a side, and picking one is the
    // resolution
    let keep_mine = use_callback(move |()| {
        if editor.write().clobber() {
            status.write().resolve(Source::Conflict);
        }
    });
    let take_disk = use_callback({
        let fragments = fragments.clone();
        move |()| {
            let file =
                editor.peek().note().map(|(path, _)| path.to_path_buf());
            editor.set(file.map_or_else(Editor::closed, Editor::open));
            vim.write().note_opened();
            fragments.borrow_mut().sweep();
            status.write().resolve(Source::Conflict);
        }
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
                palette::CommandId::SearchText => open_finder.call(()),
                // the caret commands run against the caret the palette
                // opened over — app state nothing could have moved; the
                // palette lists them only over an active block, which is
                // their guard (palette.rs `available`)
                palette::CommandId::InsertLink => open_picker.call(()),
                palette::CommandId::FollowLink => follow_at.call(()),
                palette::CommandId::OpenLoops => toggle_loops.call(()),
                palette::CommandId::OpenDaily => open_daily.call(()),
                palette::CommandId::PreviousDaily => {
                    step_time.call((NoteType::Daily, false));
                }
                palette::CommandId::NextDaily => {
                    step_time.call((NoteType::Daily, true));
                }
                palette::CommandId::OpenWeekly => open_weekly.call(()),
                palette::CommandId::PreviousWeekly => {
                    step_time.call((NoteType::Weekly, false));
                }
                palette::CommandId::NextWeekly => {
                    step_time.call((NoteType::Weekly, true));
                }
                palette::CommandId::OpenSeason => open_season.call(()),
                palette::CommandId::PreviousSeason => {
                    step_time.call((NoteType::Seasonal, false));
                }
                palette::CommandId::NextSeason => {
                    step_time.call((NoteType::Seasonal, true));
                }
                palette::CommandId::GoToTable => go_table.call(()),
                palette::CommandId::GoToLogs => go_logs.call(()),
                palette::CommandId::NewNote => open_creator.call(()),
                palette::CommandId::DeleteNote => delete_note.call(()),
                palette::CommandId::Notices => toggle_notices.call(()),
                palette::CommandId::KeepMine => keep_mine.call(()),
                palette::CommandId::TakeDisk => take_disk.call(()),
                palette::CommandId::ZoomToBodies => {
                    zoom_to.call(table::Zoom::Bodies);
                }
                palette::CommandId::ZoomToTitles => {
                    zoom_to.call(table::Zoom::Titles);
                }
                palette::CommandId::FilterCards => open_filter.call(()),
                palette::CommandId::FoldRail => {
                    fold_pane.call(keymap::Fold::Rail);
                }
                palette::CommandId::FoldJump => {
                    fold_pane.call(keymap::Fold::Jump);
                }
                palette::CommandId::OpenNote => open_switcher.call(()),
                palette::CommandId::ArrangeCluster => {
                    arrange_cluster.call(());
                }
                palette::CommandId::Undo => undo_last.call(()),
                palette::CommandId::EditTemplate => {
                    open_templates.call(());
                }
                palette::CommandId::ExportPdf => export_pdf.call(()),
                palette::CommandId::OpenSettings => open_settings.call(()),
            }
        });

    let (scale, id) = selected();
    let note_list = notes();
    let exists = note_list.iter().any(|(existing, _)| existing == &id);
    let rows = logs::rail_rows(&note_list, Some(&(scale.clone(), id.clone())));
    let crumbs = logs::breadcrumbs(&scale, &id);
    // reading the theme here re-renders every fragment when the palette's
    // "toggle theme" row fires; reading the size does the same when it
    // changes (adr/2026-08-one-font-size-for-source-and-render.md)
    let light = use_context::<Signal<bool>>();
    let theme = if light() {
        RenderTheme::Light(font_size())
    } else {
        RenderTheme::Dark(font_size())
    };
    // the settings overlay's theme row reads this, the same fact as `theme`
    // above (adr/2026-08-settings-overlay.md)
    let theme_label = if light() { "light" } else { "dark" };
    // reading the tick is what re-renders landed compiles in — the probes
    // below answer Ready only because the drain bumped this
    let _ = compiled.read();
    // the one notice line, read from the one owner: highest severity
    // standing, latest among equals (adr/2026-08-status-surface-owns-notices.md)
    let notice = status.read().line().cloned();
    // the open template, if the one editor holds one: derived from the
    // buffer's own path, never tracked beside it — it gates the pieces of
    // the logs pane that belong to the selected note
    // (adr/2026-08-template-editing-in-the-one-editor.md)
    let template_open = open_template(&editor.read(), &root);
    // the footer belongs to the logs' selected note; over the table the
    // editor holds the sheet's, whose backlinks the sheet counts itself
    let footer = (screen() == Screen::Logs && template_open.is_none())
        .then(|| link_footer(&root, &editor.read(), &id, &note_list))
        .flatten();

    // the sink's keystroke, translated by `keymap::action` and applied —
    // the only code that runs editor ops for typing; the v2 modal layer
    // slots between the translation and this (editor.rs)
    let apply_action = use_callback({
        let clipboard = clipboard.clone();
        let clipboard_image = clipboard_image.clone();
        let clipboard_write = clipboard_write.clone();
        let now = now.clone();
        let root = root.clone();
        let fragments = fragments.clone();
        move |action: keymap::Action| match action {
            keymap::Action::Insert(text) => {
                // the one door that closes its own pairs: paste and the
                // IME's commit below stay on `insert_at_caret`
                // (adr/2026-08-autopairs-in-the-typing-path.md)
                editor.write().insert_typed(&text);
                // a second `[` summons the picker, Obsidian's gesture: the
                // empty `[[]]` the pairs left is taken back out first, so
                // Escape leaves the prose as it was and accepting writes
                // the one link shape
                // (adr/2026-09-wiki-links-replace-the-l-call.md)
                let opened = (text == "[")
                    .then(|| {
                        let editor = editor.peek();
                        let (_, head) = editor.caret_in_block();
                        editor
                            .caret()
                            .zip(editor.active_source())
                            .filter(|(_, source)| {
                                links::typed_wiki_opening(source, head)
                            })
                            .map(|(caret, _)| caret.head)
                    })
                    .flatten();
                if let Some(head) = opened {
                    editor.write().splice(head - 2..head + 2, "", head - 2);
                    open_picker.call(());
                }
            }
            keymap::Action::NewLine => editor.write().insert_newline(),
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
                // will not answer pastes nothing; one that holds an image
                // and no text pastes the image's `#image` call
                if let Some(clipboard) = clipboard.clone() {
                    let paste = pasted(
                        clipboard,
                        clipboard_image.clone(),
                        now.clone(),
                        root.clone(),
                        editor
                            .peek()
                            .note()
                            .map(|(path, _)| crate::domain::stem_of(path)),
                        status,
                    );
                    spawn(async move {
                        if let Some(text) = paste.await {
                            editor.write().insert_at_caret(&text);
                        }
                    });
                }
            }
            keymap::Action::Ignore => {}
        }
    });

    // One key against the note, through the grammar. The sink only
    // exists over an open note with a caret, so the zip never comes up
    // empty; the three callers — the sink's keydown, a committed
    // composition, and search's synthesized n — all need the same
    // snapshot, and the editor guard must drop before `vim` is written.
    let grammar = use_callback(move |(key, modifiers): (Key, Modifiers)| {
        let snapshot = editor.peek();
        snapshot.note().zip(snapshot.caret()).map_or(
            vim::Outcome::Pass,
            |((_, note_text), at)| {
                vim.write().handle(
                    &key,
                    modifiers,
                    &vim::View {
                        text: note_text,
                        blocks: snapshot.blocks(),
                        head: at.head,
                        anchor: at.anchor,
                    },
                )
            },
        )
    });

    // the grammar's intents, applied in order — the executor never thinks
    // (adr/2026-08-escape-ladder-editor-wide-mode.md)
    let apply_vim = use_callback({
        let fragments = fragments.clone();
        let clipboard = clipboard.clone();
        let clipboard_image = clipboard_image.clone();
        let clipboard_write = clipboard_write.clone();
        let now = now.clone();
        let root = root.clone();
        let line_probe = line_probe.clone();
        let goal = goal.clone();
        move |acts: Vec<vim::Act>| {
            let before = editor.peek().active();
            // held across a whole j/k run, forgotten below by anything else
            let walked = acts
                .iter()
                .any(|act| matches!(act, vim::Act::WalkVisual { .. }));
            for act in acts {
                match act {
                    vim::Act::Place(at) => {
                        editor.write().place_at(at);
                    }
                    vim::Act::Type(text) => {
                        editor.write().insert_at_caret(&text);
                    }
                    vim::Act::Deactivate => {
                        // the ladder's second rung: the block renders
                        // again, and the cache drops its stale fragment
                        editor.write().deactivate();
                    }
                    // the one register is the system clipboard
                    // (adr/2026-08-one-register-the-clipboard.md)
                    vim::Act::SetClipboard(text) => {
                        if let Some(write) = clipboard_write.clone() {
                            spawn(async move { (write.0)(text).await });
                        }
                    }
                    vim::Act::Splice { span, text, caret } => {
                        editor.write().splice(span, &text, caret);
                    }
                    vim::Act::Extend(at) => {
                        editor.write().extend_to(at);
                    }
                    vim::Act::SwapEnds => {
                        editor.write().swap_ends();
                    }
                    vim::Act::Checkpoint => editor.write().checkpoint(),
                    vim::Act::Undo => editor.write().undo(),
                    vim::Act::Redo => editor.write().redo(),
                    vim::Act::OpenSearch => {
                        search_query.set(String::new());
                        shown.set(Shown::opened(""));
                        search_prompt.set(true);
                    }
                    // the same one-line prompt in the same place, wearing
                    // the other sigil; a visual : arrives with its range
                    // already spelled
                    // (adr/2026-08-ex-line-is-literal-and-global.md)
                    vim::Act::OpenEx { prefill } => {
                        shown.set(Shown::opened(&prefill));
                        ex_query.set(prefill);
                        ex_prompt.set(true);
                    }
                    // :w — the buffer reaches disk now instead of at the
                    // next debounced pause; a refusal speaks through the
                    // status surface like every other save's does
                    vim::Act::Save => {
                        editor.write().flush();
                        if let Some(trouble) = editor.write().take_trouble() {
                            status
                                .write()
                                .report(Notice::from_trouble(trouble));
                        }
                    }
                    // gf: the one follow path Ctrl+Enter, Ctrl+click and
                    // the palette already share
                    // (adr/2026-08-gf-follows-the-link.md)
                    vim::Act::FollowLink => follow_at.call(()),
                    // the nonce is what makes a bare zz move anything: the
                    // caret's key carries it, so the span remounts even
                    // when the head did not budge
                    // (adr/2026-08-scroll-anchor-is-consumed-once.md)
                    vim::Act::Scroll(anchor) => {
                        let (_, nonce) = scroll_anchor();
                        scroll_anchor.set((anchor, nonce.wrapping_add(1)));
                    }
                    // visual p: the clipboard replaces the span, and what
                    // it replaced does not go back out
                    // (adr/2026-08-visual-gains-p-r-s-and-gv.md)
                    vim::Act::PasteOver { span, linewise } => {
                        let Some(clipboard) = clipboard.clone() else {
                            continue;
                        };
                        spawn(async move {
                            let Some(clip) = clipboard_answer(
                                (clipboard.0)().await,
                                status,
                            ) else {
                                return;
                            };
                            if clip.is_empty() {
                                return;
                            }
                            // a line-wise selection swallowed its own
                            // newline, so the clip keeps its one; a
                            // char-wise span takes the text alone
                            let text = if linewise {
                                clip
                            } else {
                                clip.trim_end_matches('\n').to_string()
                            };
                            let caret = span.start
                                + caret::prev_cluster(&text, text.len());
                            editor.write().splice(span, &text, caret);
                        });
                    }
                    // the grammar cannot see the webview's wrapped lines:
                    // the whole run goes over the geometry seam in one
                    // round trip, the logical fallback taking over — and
                    // stopping the run — the moment the seam runs out of
                    // drawn lines
                    // (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md)
                    vim::Act::WalkVisual {
                        down,
                        count,
                        extend,
                    } => {
                        // every vertical move pins the caret to the pane's
                        // centre — the gjzz / gkzz the user's own vim maps
                        // (adr/2026-08-scroll-anchor-is-consumed-once.md)
                        let (_, nonce) = scroll_anchor();
                        scroll_anchor
                            .set((vim::Anchor::Center, nonce.wrapping_add(1)));
                        let count = bounded_steps(&editor.peek(), count);
                        spawn(walk_visual(
                            editor,
                            line_probe.clone(),
                            goal.clone(),
                            fragments.clone(),
                            Step {
                                down,
                                count,
                                extend,
                                held: goal.get(),
                            },
                        ));
                    }
                    // the one async act: read the clipboard, then the
                    // editor decides pure against the state the read found
                    vim::Act::Paste { before, count } => {
                        let Some(clipboard) = clipboard.clone() else {
                            continue;
                        };
                        let paste = pasted(
                            clipboard,
                            clipboard_image.clone(),
                            now.clone(),
                            root.clone(),
                            editor
                                .peek()
                                .note()
                                .map(|(path, _)| crate::domain::stem_of(path)),
                            status,
                        );
                        spawn(async move {
                            let Some(clip) = paste.await else { return };
                            // a paste is always an insertion inside the
                            // active block: no other block can wake, so no
                            // fragment goes stale
                            editor.write().paste(&clip, before, count);
                        });
                    }
                }
            }
            // a landing that woke another block — or the rung that closed
            // one — leaves a stale fragment behind
            if editor.peek().active() != before {
                fragments.borrow_mut().sweep();
            }
            // the goal column lives only across a j/k run: any other key
            // — including a WalkVisual-free vim outcome — forgets it
            if !walked {
                goal.set(goal.get().forgotten());
            }
        }
    });

    // the block panes, one closure both screens mount: the logs centre pane
    // and the writing sheet show the one editor through the one widget
    // (adr/2026-08-sheet-reuses-the-one-editor.md)
    let blocks_view = {
        let root = root.clone();
        let feed = feed.clone();
        let fragments = fragments.clone();
        let sink = sink.clone();
        let hit = hit.clone();
        let dragging = dragging.clone();
        let probing = probing.clone();
        let goal = goal.clone();
        move || -> Option<Element> {
            // V's whole-line reach has to widen the covered blocks' own
            // highlight too, not just the active line's (adr/2026-08-visual-selection-drawn-across-lines.md)
            let visual_linewise = matches!(
                vim.read().mode,
                vim::Mode::Visual(vim::VisualKind::Line)
            );
            let panes = block_panes(
                &editor.read(),
                &root,
                theme,
                &mut fragments.borrow_mut(),
                !feed.inline,
                visual_linewise,
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
                                    // the caret is the mode indicator: a
                                    // box thinking, a bar writing
                                    // (adr/2026-08-caret-shape-is-the-mode-indicator.md)
                                    let shape = match vim.read().mode {
                                        vim::Mode::Normal
                                        | vim::Mode::Visual(_)
                                        | vim::Mode::Replace => {
                                            caret::Shape::Box
                                        }
                                        vim::Mode::Insert => caret::Shape::Bar,
                                    };
                                    // V paints whole lines; v stays byte-exact
                                    // (adr/2026-08-v-highlight-covers-whole-lines.md)
                                    let linewise = matches!(
                                        vim.read().mode,
                                        vim::Mode::Visual(vim::VisualKind::Line)
                                    );
                                    // the same verdict that decides an
                                    // inactive block's rendering: CSS draws
                                    // this block's markup roles too, with
                                    // caret::layout kept as the one place
                                    // that knows where the caret, the
                                    // selection and the IME preview land
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    let draw = markup::model(&text);
                                    let lines = caret::layout(
                                        &text,
                                        anchor,
                                        head,
                                        preview.read().as_deref(),
                                        shape,
                                        linewise,
                                    );
                                    // every piece tagged with the role it
                                    // renders under, or untagged on the
                                    // Typst verdict — tagged as `None` so
                                    // the `if let` below omits the class
                                    // attribute outright, leaving the
                                    // fallback's markup byte-for-byte what
                                    // it was before this block drew styled
                                    let rendered_lines: TintedLines = match &draw
                                    {
                                        markup::Draw::Css(model) => lines
                                            .into_iter()
                                            .map(|line| {
                                                markup::tint(
                                                    &model.spans,
                                                    line.pieces,
                                                )
                                                .into_iter()
                                                .map(|(role, delimiter, piece)| {
                                                    (Some((role, delimiter)), piece)
                                                })
                                                .collect()
                                            })
                                            .collect(),
                                        markup::Draw::Typst => lines
                                            .into_iter()
                                            .map(|line| {
                                                line.pieces
                                                    .into_iter()
                                                    .map(|piece| (None, piece))
                                                    .collect()
                                            })
                                            .collect(),
                                    };
                                    // the block's structural role
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    // — `None` on the Typst verdict, since a
                                    // fallback block has no markup block
                                    // role to draw, leaving `.block-active`
                                    // exactly what it drew before this task
                                    let block_class = match &draw {
                                        markup::Draw::Css(model) => {
                                            Some(markup::block_class(model.block))
                                        }
                                        markup::Draw::Typst => None,
                                    };
                                    // a nested list item's indent, same
                                    // treatment as `block_class` above
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    let item_style = match &draw {
                                        markup::Draw::Css(model) => {
                                            markup::item_indent_style(model.block)
                                        }
                                        markup::Draw::Typst => None,
                                    };
                                    rsx! {
                                        div {
                                            key: "{start}",
                                            class: "block-active",
                                            class: if let Some(bc) = &block_class { "{bc}" },
                                            style: if let Some(s) = &item_style { "{s}" },
                                            // a press asks the hit probe which character it
                                            // landed on; Ctrl makes it a follow, like
                                            // Ctrl+Enter (adr/2026-08-ctrl-enter-opens-time-links.md)
                                            onmousedown: {
                                                let hit = hit.clone();
                                                let dragging = dragging.clone();
                                                let goal = goal.clone();
                                                move |event: MouseEvent| {
                                                    // a mouse-driven caret move ends any j/k
                                                    // run, the way `place_in_block` drops the
                                                    // editor's own logical goal
                                                    goal.set(goal.get().forgotten());
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
                                                let goal = goal.clone();
                                                move |event: MouseEvent| {
                                                    if !dragging.get() {
                                                        return;
                                                    }
                                                    goal.set(goal.get().forgotten());
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
                                            for (row, line) in rendered_lines.into_iter().enumerate() {
                                                div { key: "{row}", class: "source-line",
                                                    for (tag, piece) in line {
                                                        {
                                                            // the piece's own existing class stays exactly what it
                                                            // was; the role only ever adds a second class, and the
                                                            // Typst verdict's `None` tag makes that addition a
                                                            // no-op, so the fallback still draws byte-for-byte
                                                            // what it did before this block drew styled
                                                            let markup_class = tag.map(|(role, delimiter)| {
                                                                markup::class(role, delimiter)
                                                            });
                                                            match piece {
                                                                caret::Piece::Text { start, text } => rsx! {
                                                                    span {
                                                                        key: "{start}-",
                                                                        class: if let Some(mc) = markup_class { "{mc}" },
                                                                        "data-start": "{start}",
                                                                        "{text}"
                                                                    }
                                                                },
                                                                caret::Piece::Selected { start, text } => rsx! {
                                                                    span {
                                                                        key: "{start}-sel",
                                                                        class: "sel",
                                                                        class: if let Some(mc) = markup_class { "{mc}" },
                                                                        "data-start": "{start}",
                                                                        "{text}"
                                                                    }
                                                                },
                                                                caret::Piece::Preview { start, text } => rsx! {
                                                                    span {
                                                                        key: "{start}-compose",
                                                                        class: "compose",
                                                                        class: if let Some(mc) = markup_class { "{mc}" },
                                                                        "data-start": "{start}",
                                                                        "{text}"
                                                                    }
                                                                },
                                                                // both carets: keyed by position *and* by the
                                                                // scroll nonce, so every move — and every zz,
                                                                // which moves nothing — remounts them,
                                                                // restarting the bar's blink (solid while
                                                                // typing) and the scroll-into-view. No markup
                                                                // class here either — the caret's own classes
                                                                // stay untouched, since the `j`/`k` line walk
                                                                // and the mouse hit probe key off them
                                                                // (adr/2026-08-scroll-anchor-is-consumed-once.md,
                                                                // adr/2026-08-css-draws-the-markup.md)
                                                                caret::Piece::Caret => rsx! {
                                                                    span {
                                                                        key: "caret-{head}-{scroll_anchor().1}",
                                                                        class: "caret",
                                                                        onmounted: move |event: Event<MountedData>| async move {
                                                                            settle_caret(event, scroll_anchor).await;
                                                                        },
                                                                    }
                                                                },
                                                                caret::Piece::CaretBox { start, cluster } => rsx! {
                                                                    span {
                                                                        key: "caret-{head}-{scroll_anchor().1}",
                                                                        class: "caret-box",
                                                                        "data-start": "{start}",
                                                                        onmounted: move |event: Event<MountedData>| async move {
                                                                            settle_caret(event, scroll_anchor).await;
                                                                        },
                                                                        "{cluster}"
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
                                                onkeydown: {
                                                    let goal = goal.clone();
                                                    move |event: KeyboardEvent| {
                                                    // never touch a composing keystroke: the
                                                    // IME owns it, and an open preview means
                                                    // the IME owns it whatever isComposing
                                                    // says (the spike saw both)
                                                    let owned = *composing.peek();
                                                    if owned == Composing::Closing {
                                                        composing.set(Composing::No);
                                                    }
                                                    if event.data().is_composing()
                                                        || event.key() == Key::Dead
                                                        || owned != Composing::No
                                                    {
                                                        return;
                                                    }
                                                    // typed at an overlay that had not yet
                                                    // taken the focus: relayed into it, and
                                                    // stopped here so the pane does not
                                                    // relay it a second time
                                                    if relay.call((event.key(), event.modifiers())) {
                                                        event.stop_propagation();
                                                        return;
                                                    }
                                                    // the pane folds are the logs' and normal
                                                    // mode's: insert mode keeps every alt
                                                    // character typeable (AltGr), and the
                                                    // grammar would swallow the chord inert
                                                    // before the pane saw it
                                                    // (adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md)
                                                    if vim.peek().mode == vim::Mode::Normal
                                                        && *screen.peek() == Screen::Logs
                                                        && let Some(fold) = keymap::fold(&event.key(), event.modifiers())
                                                    {
                                                        event.prevent_default();
                                                        event.stop_propagation();
                                                        fold_pane.call(fold);
                                                        return;
                                                    }
                                                    // the grammar speaks first (editor.rs
                                                    // names this slot); Pass hands the key to
                                                    // the phase-0 keymap unchanged
                                                    let outcome = grammar
                                                        .call((event.key(), event.modifiers()));
                                                    match outcome {
                                                        vim::Outcome::Acts(acts) => {
                                                            event.prevent_default();
                                                            // shift+Escape leaves the note and
                                                            // keeps travelling: the table pane
                                                            // closes the sheet behind it
                                                            // (adr/2026-08-shift-escape-leaves-the-note.md)
                                                            let leaving = event.key() == Key::Escape
                                                                && event.modifiers().shift();
                                                            if !leaving {
                                                                event.stop_propagation();
                                                            }
                                                            apply_vim.call(acts);
                                                        }
                                                        // unbound normal-mode keys are inert:
                                                        // consumed, never inserted — but a key
                                                        // the grammar swallows is still another
                                                        // key, and the goal column lives only
                                                        // across a j/k run, so it forgets too.
                                                        // The one exception is a count still
                                                        // accumulating: vim's curswant survives
                                                        // one, so the 2 of 2j does not throw
                                                        // away the column the j before it set.
                                                        vim::Outcome::Swallow => {
                                                            event.prevent_default();
                                                            event.stop_propagation();
                                                            if !vim.peek().counting() {
                                                                goal.set(goal.get().forgotten());
                                                            }
                                                        }
                                                        vim::Outcome::Pass => {
                                                            // not the grammar's: Escape and the
                                                            // app chords bubble as they always did
                                                            if let Some(action) = keymap::action(&event.key(), event.modifiers()) {
                                                                // ours: keep the default out of the
                                                                // sink and the key off the pane
                                                                event.prevent_default();
                                                                event.stop_propagation();
                                                                apply_action.call(action);
                                                            }
                                                        }
                                                    }
                                                }},
                                                oncompositionstart: move |_| {
                                                    composing.set(Composing::Open);
                                                    // the IME writes; normal mode does not
                                                    if vim.peek().mode == vim::Mode::Insert {
                                                        preview.set(Some(String::new()));
                                                    }
                                                },
                                                oncompositionupdate: move |event: Event<CompositionData>| {
                                                    if vim.peek().mode == vim::Mode::Insert {
                                                        preview.set(Some(event.data().data()));
                                                    }
                                                },
                                                oncompositionend: move |event: Event<CompositionData>| {
                                                    // WebKitGTK can fire an empty end before
                                                    // the real one (the spike's transcript), so
                                                    // the early end commits nothing
                                                    preview.set(None);
                                                    let committed = event.data().data();
                                                    if committed.is_empty() {
                                                        composing.set(Composing::Closing);
                                                        return;
                                                    }
                                                    composing.set(Composing::No);
                                                    if vim.peek().mode == vim::Mode::Insert {
                                                        editor.write().insert_at_caret(&committed);
                                                        return;
                                                    }
                                                    // outside insert the composition wrote
                                                    // nothing, so its commit is the only way a
                                                    // dead key ever reaches the grammar — ^ is
                                                    // dead on a French layout, and ^ is a
                                                    // motion. One cluster is one keystroke;
                                                    // anything longer is a real IME's and stays
                                                    // discarded whole
                                                    // (adr/2026-08-normal-mode-compositions-reach-the-grammar.md)
                                                    if caret::next_cluster(&committed, 0)
                                                        != committed.len()
                                                    {
                                                        return;
                                                    }
                                                    if let vim::Outcome::Acts(acts) = grammar
                                                        .call((Key::Character(committed), Modifiers::empty()))
                                                    {
                                                        apply_vim.call(acts);
                                                    }
                                                },
                                            }
                                        }
                                    }
                                }
                                // a block the visual selection reaches
                                // past the active one: raw source, same
                                // typography, highlighted where the
                                // selection covers it, but no caret and no
                                // textarea socket — only the active block
                                // is ever the widget
                                // (adr/2026-08-visual-selection-drawn-across-lines.md)
                                Pane::Selected { start, lines, text } => {
                                    // same verdict, same tint: a covered
                                    // block draws its markup roles too, not
                                    // just its highlight
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    let draw = markup::model(&text);
                                    // the block's structural role, same
                                    // treatment as `.block-active` above:
                                    // `None` on the Typst verdict leaves
                                    // `.block-selected` exactly what it
                                    // drew before this task
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    let block_class = match &draw {
                                        markup::Draw::Css(model) => {
                                            Some(markup::block_class(model.block))
                                        }
                                        markup::Draw::Typst => None,
                                    };
                                    // a nested list item's indent, same
                                    // treatment as `block_class` above
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    let item_style = match &draw {
                                        markup::Draw::Css(model) => {
                                            markup::item_indent_style(model.block)
                                        }
                                        markup::Draw::Typst => None,
                                    };
                                    let rendered_lines: TintedLines = match &draw
                                    {
                                        markup::Draw::Css(model) => lines
                                            .into_iter()
                                            .map(|line| {
                                                markup::tint(
                                                    &model.spans,
                                                    line.pieces,
                                                )
                                                .into_iter()
                                                .map(|(role, delimiter, piece)| {
                                                    (Some((role, delimiter)), piece)
                                                })
                                                .collect()
                                            })
                                            .collect(),
                                        markup::Draw::Typst => lines
                                            .into_iter()
                                            .map(|line| {
                                                line.pieces
                                                    .into_iter()
                                                    .map(|piece| (None, piece))
                                                    .collect()
                                            })
                                            .collect(),
                                    };
                                    rsx! {
                                    div {
                                        key: "{start}",
                                        class: "block-selected",
                                        class: if let Some(bc) = &block_class { "{bc}" },
                                        style: if let Some(s) = &item_style { "{s}" },
                                        onclick: {
                                            let fragments = fragments.clone();
                                            let goal = goal.clone();
                                            move |_| {
                                                goal.set(goal.get().forgotten());
                                                editor.write().activate(start);
                                                fragments.borrow_mut().sweep();
                                            }
                                        },
                                        div { class: "selected-source",
                                            for (row, line) in rendered_lines.into_iter().enumerate() {
                                                div { key: "{row}", class: "source-line",
                                                    for (tag, piece) in line {
                                                        {
                                                            let (selected, piece_start, text) = piece_span(piece);
                                                            let markup_class = tag.map(|(role, delimiter)| {
                                                                markup::class(role, delimiter)
                                                            });
                                                            rsx! {
                                                                span {
                                                                    key: "{piece_start}-{selected}",
                                                                    class: if selected { "sel" },
                                                                    class: if let Some(mc) = markup_class { "{mc}" },
                                                                    "data-start": "{piece_start}",
                                                                    "{text}"
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
                                // a block CSS can draw: the markup model's
                                // structural role and spans laid out as
                                // styled DOM text rather than a compiled
                                // SVG (adr/2026-08-css-draws-the-markup.md)
                                Pane::Css { start, block, spans, text } => {
                                    let class = markup::block_class(block);
                                    // a blank block carries the same
                                    // shape class an active blank line
                                    // sits inside, so entering or leaving
                                    // it never shifts anything below
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    let blank = block == markup::BlockRole::Blank;
                                    // a nested list item's indent
                                    // (adr/2026-08-css-draws-the-markup.md)
                                    let item_style = markup::item_indent_style(block);
                                    let lines = markup_lines(&text, &spans);
                                    rsx! {
                                        div {
                                            key: "{start}",
                                            class: "block block-css {class}",
                                            class: if blank { "block-blank" },
                                            style: if let Some(s) = &item_style { "{s}" },
                                            onclick: {
                                                let fragments = fragments.clone();
                                                let goal = goal.clone();
                                                move |_| {
                                                    goal.set(goal.get().forgotten());
                                                    editor.write().activate(start);
                                                    fragments.borrow_mut().sweep();
                                                }
                                            },
                                            div { class: "block-source",
                                                for (row, line) in lines.into_iter().enumerate() {
                                                    div { key: "{row}", class: "source-line",
                                                        for (role, delimiter, piece) in line {
                                                            {
                                                                let (piece_start, piece_text) = css_piece_span(piece);
                                                                let span_class = markup::class(role, delimiter);
                                                                rsx! {
                                                                    span {
                                                                        key: "{piece_start}",
                                                                        class: "{span_class}",
                                                                        "data-start": "{piece_start}",
                                                                        "{piece_text}"
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
                                Pane::Fragment { start, rendered } => rsx! {
                                    div {
                                        key: "{start}",
                                        class: "block block-svg",
                                        onclick: {
                                            let fragments = fragments.clone();
                                            let goal = goal.clone();
                                            move |_| {
                                                goal.set(goal.get().forgotten());
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
                                Pane::Pending { start, text, job, shelved } => {
                                    // the compile rides the tier (at most
                                    // once — the probe dedups) while the
                                    // slot shows what it last showed: the
                                    // previous SVG of a block whose content
                                    // changed, dimmed as stale, or the raw
                                    // source dimmed when the slot never
                                    // showed one
                                    // (adr/2026-09-fragments-shelve-their-last-svg-per-block.md,
                                    // adr/2026-08-async-caches-pending-stale.md).
                                    if let Some(job) = job {
                                        (feed.submit)(Job::Fragment(job));
                                    }
                                    let stale = shelved.is_some();
                                    rsx! {
                                        div {
                                            key: "{start}",
                                            class: "block block-svg block-pending",
                                            class: if stale { "block-stale" },
                                            onclick: {
                                                let fragments = fragments.clone();
                                                let goal = goal.clone();
                                                move |_| {
                                                    goal.set(goal.get().forgotten());
                                                    editor.write().activate(start);
                                                    fragments.borrow_mut().sweep();
                                                }
                                            },
                                            {
                                                match shelved {
                                                    Some(svg) => rsx! {
                                                        div { class: "note", dangerous_inner_html: "{svg}" }
                                                    },
                                                    None => rsx! {
                                                        div { class: "pending-source", "{text}" }
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }
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
                        value: "{query}",
                        placeholder: "link to…",
                        onmounted: move |event| async move {
                            let _ = event.set_focus(true).await;
                        },
                        oninput: move |event| {
                            query.set(typed.call((query, event.value())));
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
    // the / prompt, the picker's little sibling: one input, Enter commits
    // the pattern and jumps as n would, Escape backs out
    // (adr/2026-08-search-lands-through-place.md)
    let search_view = move || -> Element {
        if !search_prompt() {
            return rsx! {};
        }
        rsx! {
            div { class: "link-picker",
                input {
                    class: "picker-query",
                    value: "{search_query}",
                    placeholder: "/",
                    onmounted: move |event| async move {
                        let _ = event.set_focus(true).await;
                    },
                    oninput: move |event| search_query.set(typed.call((search_query, event.value()))),
                    onkeydown: move |event: KeyboardEvent| {
                        match event.key() {
                            Key::Escape => search_prompt.set(false),
                            Key::Enter => {
                                let pattern = search_query.peek().clone();
                                search_prompt.set(false);
                                vim.write().commit_search(pattern);
                                // the first jump is n's, synthesized
                                let outcome = grammar.call((
                                    Key::Character("n".to_string()),
                                    Modifiers::empty(),
                                ));
                                if let vim::Outcome::Acts(acts) = outcome {
                                    apply_vim.call(acts);
                                }
                            }
                            _ => {}
                        }
                        // the prompt owns every plain key while it is
                        // open; the ctrl chords still bubble
                        if !event.modifiers().ctrl() {
                            event.stop_propagation();
                        }
                    },
                }
            }
        }
    };

    // the : prompt, the / prompt's twin: the same widget in the same
    // region, wearing the other sigil, so the one place a one-line command
    // appears stays the one place (AIR LAY-1). Enter resolves it in the
    // grammar and Escape backs out leaving the caret and the selection
    // exactly as they stood (AIR ERR-6,
    // adr/2026-08-ex-line-is-literal-and-global.md)
    let ex_view = move || -> Element {
        if !ex_prompt() {
            return rsx! {};
        }
        rsx! {
            div { class: "link-picker",
                input {
                    class: "picker-query",
                    placeholder: ":",
                    value: "{ex_query}",
                    onmounted: move |event| async move {
                        let _ = event.set_focus(true).await;
                    },
                    oninput: move |event| ex_query.set(typed.call((ex_query, event.value()))),
                    onkeydown: move |event: KeyboardEvent| {
                        match event.key() {
                            Key::Escape => ex_prompt.set(false),
                            Key::Enter => {
                                let line = ex_query.peek().clone();
                                ex_prompt.set(false);
                                // the same shape the grammar callback uses:
                                // the prompt only opens over a note, so the
                                // zip never comes up empty, and the editor
                                // guard drops before an act writes it back
                                let resolved = {
                                    let snapshot = editor.peek();
                                    snapshot.note().zip(snapshot.caret()).map_or(
                                        vim::ExOutcome::Acts(Vec::new()),
                                        |((_, note_text), at)| {
                                            vim.write().commit_ex(
                                                &line,
                                                &vim::View {
                                                    text: note_text,
                                                    blocks: snapshot.blocks(),
                                                    head: at.head,
                                                    anchor: at.anchor,
                                                },
                                            )
                                        },
                                    )
                                };
                                // a line that could not be read says so
                                // rather than vanishing (AIR ERR-2)
                                match resolved {
                                    vim::ExOutcome::Acts(acts) => {
                                        apply_vim.call(acts);
                                    }
                                    vim::ExOutcome::Refused(reason) => {
                                        status
                                            .write()
                                            .report(Notice::ex_refused(&reason));
                                    }
                                }
                            }
                            _ => {}
                        }
                        // the prompt owns every plain key while it is
                        // open; the ctrl chords still bubble
                        if !event.modifiers().ctrl() {
                            event.stop_propagation();
                        }
                    },
                }
            }
        }
    };

    // the palette's rows, cloned out the same way; which commands exist at
    // all was decided at open (adr/2026-08-palette-birth-command-list.md)
    // the edit-template picker, the jump overlay's grammar over a
    // directory listing (adr/2026-08-template-editing-in-the-one-editor.md)
    let template_view = move || -> Element {
        match template_picker() {
            Some(frozen) => {
                let needle = template_query.read().to_lowercase();
                let rows: Vec<String> = frozen
                    .entries
                    .iter()
                    .filter(|name| name.to_lowercase().contains(&needle))
                    .cloned()
                    .collect();
                let keys_rows = rows.clone();
                rsx! {
                div { class: "command-palette",
                    div { class: "palette-head type-label", "edit template" }
                    input {
                        class: "picker-query",
                        value: "{template_query}",
                        placeholder: "template…",
                        onmounted: move |event| async move {
                            let _ = event.set_focus(true).await;
                        },
                        oninput: move |event| {
                            template_query.set(typed.call((template_query, event.value())));
                            template_highlighted.set(0);
                        },
                        onkeydown: move |event: KeyboardEvent| {
                            let key = event.key();
                            let last = keys_rows.len().saturating_sub(1);
                            match key {
                                Key::Escape => close_templates.call(()),
                                Key::Enter => {
                                    // no matches: the keystroke does
                                    // nothing rather than guessing
                                    if let Some(name) = keys_rows.get(template_highlighted()) {
                                        edit_template.call(name.clone());
                                    }
                                }
                                Key::ArrowDown => {
                                    template_highlighted.set((template_highlighted() + 1).min(last));
                                }
                                Key::ArrowUp => {
                                    template_highlighted.set(template_highlighted().saturating_sub(1));
                                }
                                _ => {}
                            }
                            if !event.modifiers().ctrl() {
                                event.stop_propagation();
                            }
                        },
                    }
                    if rows.is_empty() {
                        div { class: "picker-empty", "no matching template" }
                    }
                    for (rank, name) in rows.into_iter().enumerate() {
                        div {
                            key: "{name}",
                            class: "picker-row",
                            class: if rank == template_highlighted() { "selected" },
                            onclick: {
                                let name = name.clone();
                                move |_| edit_template.call(name.clone())
                            },
                            span { class: "picker-id", "{name}" }
                        }
                    }
                }
                }
            }
            None => rsx! {},
        }
    };
    let open_palette = palette().map(|frozen| {
        let matches = palette::filter(
            &palette_query.read(),
            palette::Context {
                block_active: frozen.block_active,
                note_open: frozen.note_open,
                on_table: frozen.on_table,
                sheet_open: frozen.sheet_open,
                at_bodies: frozen.at_bodies,
                conflict: frozen.conflict,
                undoable: frozen.undoable,
            },
        );
        // the undo row wears the register's words for what it would take
        // back; every other row keeps its registry label
        // (adr/2026-08-app-level-undo-register.md)
        let rows: Vec<(&palette::Command, String)> = matches
            .into_iter()
            .map(|command| {
                let label = match command.id {
                    palette::CommandId::Undo => undo_register.read().label(),
                    _ => None,
                }
                .unwrap_or_else(|| command.label.to_string());
                (command, label)
            })
            .collect();
        (frozen, rows)
    });
    // the creator's rows, the same clone-out; only step 1 has rows to filter
    let open_creator_view = creator().map(|frozen| {
        let matches = crate::create::filter(&creator_query.read());
        let notice = creator_notice();
        (frozen, matches, notice)
    });
    let captured =
        (exists && scale == NoteType::Daily && template_open.is_none())
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
            let line = table::tether(
                card.x,
                card.y,
                pan(),
                table::sheet_frame(viewport()),
            );
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
        move |event: KeyboardEvent| {
            // read before the match: a guard's borrow would still be held
            // when an arm writes the editor back
            let over_template = open_template(&editor.peek(), &root).is_some();
            match event.key() {
                // typed at an overlay that had not yet taken the focus:
                // relayed into it, never read as a rung or a chord here
                key if relay.call((key.clone(), event.modifiers())) => {
                    event.stop_propagation();
                }
                // shift+Escape belongs to the note it just left; the rungs
                // below are plain Escape's, and a note must not acknowledge
                // a notice on its way out
                // (adr/2026-08-shift-escape-leaves-the-note.md)
                Key::Escape if event.modifiers().shift() => {}
                // the settings overlay closes first, above the notices and
                // loops rungs — defence in depth behind its own onkeydown
                // (adr/2026-08-settings-overlay.md)
                Key::Escape if settings_open() => settings_open.set(false),
                // the notices overlay is a destination you leave, like the
                // loops list below it on the ladder
                Key::Escape if notices_open() => notices_open.set(false),
                // the open-loops list is a destination you leave; escape
                // reaches here only when no block owns it
                Key::Escape if loops_open() => loops_open.set(false),
                // an open template is a destination you leave too: the
                // centre pane goes back to the selected note
                // (adr/2026-08-template-editing-in-the-one-editor.md)
                Key::Escape if over_template => {
                    // a return to the note already standing, not a new
                    // visit — `edit_template` itself never pushes on the
                    // way in, so `select` must not push on the way back
                    // out either (adr/2026-08-note-history-back.md)
                    let target = selected.peek().clone();
                    restoring_history.set(true);
                    select.call(target);
                    restoring_history.set(false);
                }
                // the ladder's bottom: with nothing left to close, Escape
                // acknowledges the visible notice — the explicit gesture a
                // critical requires (adr/2026-08-status-surface-owns-notices.md);
                // gated so a bare Escape over a clean line writes nothing
                Key::Escape => {
                    if status.peek().line().is_some() {
                        status.write().acknowledge();
                    }
                }
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
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !settings_open()
                        && editor.peek().active().is_some() =>
                {
                    open_picker.call(());
                }
                // the todo toggle (adr/2026-08-ctrl-t-toggles-the-todo.md):
                // buffer-level like Ctrl+L, not a `keymap::Action` — it
                // edits the line the caret sits on, never the grammar.
                // Guarded like every other chord that reaches the buffer or
                // the pane: an overlay owns the keyboard while it stands,
                // so the chord must not rewrite the line behind it
                Key::Character(ref character)
                    if character == "t"
                        && event.modifiers().ctrl()
                        && editor.peek().active().is_some()
                        && palette.peek().is_none()
                        && picker.peek().is_none()
                        && creator.peek().is_none()
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !settings_open() =>
                {
                    event.prevent_default();
                    editor.write().toggle_todo();
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
                        && creator.peek().is_none()
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !settings_open()
                        && !notices_open()
                        && !loops_open() =>
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
                        && picker.peek().is_none()
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !settings_open() =>
                {
                    // the webview's own Ctrl+N would open a window
                    event.prevent_default();
                    open_creator.call(());
                }
                // the palette's daily command's chord (the palette's own
                // OpenDaily), guarded like the palette and the create
                // overlay: overlays never stack. `!shift()` keeps this arm
                // from also answering Ctrl+Shift+D, the table arm's twin
                // (adr/2026-08-delete-note-chord.md)
                Key::Character(ref character)
                    if character == "d"
                        && event.modifiers().ctrl()
                        && !event.modifiers().shift()
                        && palette.peek().is_none()
                        && picker.peek().is_none()
                        && creator.peek().is_none()
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !settings_open() =>
                {
                    // the webview's own Ctrl+D would open a bookmark dialog
                    event.prevent_default();
                    open_daily.call(());
                }
                // the settings overlay (adr/2026-08-settings-overlay.md),
                // guarded the same way ctrl+d is: overlays never stack
                Key::Character(ref character)
                    if character == ","
                        && event.modifiers().ctrl()
                        && palette.peek().is_none()
                        && picker.peek().is_none()
                        && creator.peek().is_none()
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !notices_open()
                        && !loops_open() =>
                {
                    event.prevent_default();
                    open_settings.call(());
                }
                // Alt+H / Alt+L fold the panes from the pane itself — the
                // empty day, where no sink holds the keys
                // (adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md)
                Key::Character(_)
                    if keymap::fold(&event.key(), event.modifiers())
                        .is_some()
                        && palette.peek().is_none()
                        && creator.peek().is_none()
                        && switcher.peek().is_none()
                        && !finder_open()
                        && !settings_open() =>
                {
                    event.prevent_default();
                    // the guard proved the chord; the default is never
                    // the value taken
                    fold_pane.call(
                        keymap::fold(&event.key(), event.modifiers())
                            .unwrap_or(keymap::Fold::Rail),
                    );
                }
                // Ctrl+O, the note switcher — the same chord the table
                // answers, because the switcher belongs to no screen
                // (adr/2026-09-ctrl-o-is-the-one-note-switcher.md);
                // guarded like every other overlay-aware chord here:
                // overlays never stack
                Key::Character(ref character)
                    if character == "o"
                        && event.modifiers().ctrl()
                        && palette.peek().is_none()
                        && picker.peek().is_none()
                        && creator.peek().is_none()
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !finder_open()
                        && !settings_open() =>
                {
                    // the webview owns Ctrl+O as an open dialog
                    event.prevent_default();
                    open_switcher.call(());
                }
                // Ctrl+Shift+F, the finder over the vault's text; the
                // shifted key arrives upper-case
                // (adr/2026-09-full-text-search-lives-in-the-index.md)
                Key::Character(ref character)
                    if character.eq_ignore_ascii_case("f")
                        && event.modifiers().ctrl()
                        && event.modifiers().shift()
                        && palette.peek().is_none()
                        && picker.peek().is_none()
                        && creator.peek().is_none()
                        && template_picker.peek().is_none()
                        && switcher.peek().is_none()
                        && !finder_open()
                        && !settings_open() =>
                {
                    event.prevent_default();
                    open_finder.call(());
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
                // only enter writes the file — navigating never does. Over a
                // note that already stands it is the way back in instead,
                // the caret returning where it was left
                // (adr/2026-08-enter-returns-to-the-note.md). It reaches
                // here only with no block active: an active one owns Enter.
                Key::Enter => {
                    let (scale, id) = selected();
                    if notes.read().iter().any(|(existing, _)| existing == &id)
                    {
                        editor.write().reactivate();
                        return;
                    }
                    if create_time_note.call((scale.clone(), id.clone())) {
                        select.call((scale, id));
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
            // typed at an overlay that had not yet taken the focus:
            // relayed into it, never read as a rung or a chord here
            key if relay.call((key.clone(), event.modifiers())) => {
                event.stop_propagation();
            }
            // the note's own exit gesture, arriving from the block it just
            // left: the sheet is the note here, so it goes with it
            // (adr/2026-08-shift-escape-leaves-the-note.md)
            Key::Escape if event.modifiers().shift() => {
                if sheet.peek().is_some() {
                    close_sheet.call(());
                }
            }
            // the settings overlay closes first, above the notices rung —
            // defence in depth behind its own onkeydown
            // (adr/2026-08-settings-overlay.md)
            Key::Escape if settings_open() => settings_open.set(false),
            // the notices overlay closes before the sheet: overlays leave
            // the ladder first
            Key::Escape if notices_open() => notices_open.set(false),
            // the open-loops list is a destination you leave too — the
            // table pane's twin of the logs arm's rung, so the ember and
            // the palette's open-loops command both close from here
            Key::Escape if loops_open() => loops_open.set(false),
            // the ladder's bottom, the logs arm's twin: acknowledge the
            // visible notice, gated so a clean line writes nothing
            Key::Escape => {
                if status.peek().line().is_some() {
                    status.write().acknowledge();
                }
            }
            // the link picker over the sheet's active block — the logs
            // pane's arm, guard for guard (adr/2026-08-ctrl-l-link-picker.md)
            Key::Character(ref character)
                if character == "l"
                    && event.modifiers().ctrl()
                    && picker.peek().is_none()
                    && !settings_open()
                    && editor.peek().active().is_some() =>
            {
                open_picker.call(());
            }
            // the todo toggle, the logs arm's twin, guarded like every
            // other overlay-aware chord here: an overlay owns the keyboard
            // while it stands
            // (adr/2026-08-ctrl-t-toggles-the-todo.md)
            Key::Character(ref character)
                if character == "t"
                    && event.modifiers().ctrl()
                    && editor.peek().active().is_some()
                    && palette.peek().is_none()
                    && picker.peek().is_none()
                    && creator.peek().is_none()
                    && filter_picker.peek().is_none()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !settings_open() =>
            {
                event.prevent_default();
                editor.write().toggle_todo();
            }
            Key::Character(ref character)
                if character == "p"
                    && event.modifiers().ctrl()
                    && palette.peek().is_none()
                    && picker.peek().is_none()
                    && creator.peek().is_none()
                    && filter_picker.peek().is_none()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !settings_open()
                    && !notices_open()
                    && !loops_open() =>
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
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !settings_open() =>
            {
                // the webview's own Ctrl+N would open a window
                event.prevent_default();
                open_creator.call(());
            }
            // the palette's daily command's chord, the logs arm's twin —
            // landing on the daily means landing on the temporal screen too
            // (todo 25), so the table closes under it first; `!shift()`
            // keeps this arm from also answering Ctrl+Shift+D
            // (adr/2026-08-delete-note-chord.md)
            Key::Character(ref character)
                if character == "d"
                    && event.modifiers().ctrl()
                    && !event.modifiers().shift()
                    && palette.peek().is_none()
                    && picker.peek().is_none()
                    && creator.peek().is_none()
                    && filter_picker.peek().is_none()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !settings_open() =>
            {
                // the webview's own Ctrl+D would open a bookmark dialog
                event.prevent_default();
                // `select` first: with a sheet open it records the sheet
                // on the visit log before closing it — `go_logs` leading
                // closed the sheet and made the log record the logs
                // selection underneath instead
                open_daily.call(());
                go_logs.call(());
            }
            // Ctrl+Shift+D deletes the open sheet's note immediately, no
            // confirmation (adr/2026-08-delete-note-chord.md). Guarded on
            // `sheet.peek().is_some()`, the palette's own visibility rule
            // for "delete note" — the sheet never stands open behind the
            // logs screen, so this chord is wired here only.
            Key::Character(ref character)
                if character.eq_ignore_ascii_case("d")
                    && event.modifiers().ctrl()
                    && event.modifiers().shift()
                    && sheet.peek().is_some()
                    && palette.peek().is_none()
                    && picker.peek().is_none()
                    && creator.peek().is_none()
                    && filter_picker.peek().is_none()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !settings_open() =>
            {
                event.prevent_default();
                delete_note.call(());
            }
            // the settings overlay, the logs arm's twin
            // (adr/2026-08-settings-overlay.md)
            Key::Character(ref character)
                if character == ","
                    && event.modifiers().ctrl()
                    && palette.peek().is_none()
                    && picker.peek().is_none()
                    && creator.peek().is_none()
                    && filter_picker.peek().is_none()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !notices_open()
                    && !loops_open() =>
            {
                event.prevent_default();
                open_settings.call(());
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
            // the card filter, table-only, guarded like every overlay
            // chord (adr/2026-08-filter-overlay-ctrl-f.md)
            Key::Character(ref character)
                if character == "f"
                    && event.modifiers().ctrl()
                    && !event.modifiers().shift()
                    && filter_picker.peek().is_none()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !finder_open()
                    && palette.peek().is_none()
                    && creator.peek().is_none()
                    && picker.peek().is_none()
                    && !settings_open() =>
            {
                // the webview owns Ctrl+F as find-in-page
                event.prevent_default();
                open_filter.call(());
            }
            // Ctrl+Shift+F, the finder over the vault's text, the logs
            // arm's twin (adr/2026-09-full-text-search-lives-in-the-index.md)
            Key::Character(ref character)
                if character.eq_ignore_ascii_case("f")
                    && event.modifiers().ctrl()
                    && event.modifiers().shift()
                    && filter_picker.peek().is_none()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && !finder_open()
                    && palette.peek().is_none()
                    && creator.peek().is_none()
                    && picker.peek().is_none()
                    && !settings_open() =>
            {
                event.prevent_default();
                open_finder.call(());
            }
            // Ctrl+O, the note switcher, the logs arm's twin
            // (adr/2026-09-ctrl-o-is-the-one-note-switcher.md)
            Key::Character(ref character)
                if character == "o"
                    && event.modifiers().ctrl()
                    && template_picker.peek().is_none()
                    && switcher.peek().is_none()
                    && filter_picker.peek().is_none()
                    && palette.peek().is_none()
                    && creator.peek().is_none()
                    && picker.peek().is_none()
                    && !finder_open()
                    && !settings_open() =>
            {
                // the webview owns Ctrl+O as an open dialog
                event.prevent_default();
                open_switcher.call(());
            }
            _ => {}
        }
    };

    rsx! {
        Chrome {
            screen: screen(),
            loops: loops.read().len(),
            filter: filter.read().as_ref().map(table::filter_label),
            liveness: status.read().liveness(),
            // overlays never stack (adr/2026-08-settings-overlay.md): the
            // chrome sits above the floating overlays' box, so the ember
            // stays clickable while one is up and must go inert instead
            on_ember: move |_| {
                if !settings_open() && !notices_open() {
                    toggle_loops.call(());
                }
            },
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
                            value: "{palette_query}",
                            placeholder: "command…",
                            onmounted: move |event| async move {
                                let _ = event.set_focus(true).await;
                            },
                            oninput: move |event| {
                                palette_query.set(typed.call((palette_query, event.value())));
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
                                        if let Some((command, _)) = matches.get(palette_highlighted()) {
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
                        for (rank, (command, label)) in rows.into_iter().enumerate() {
                            div {
                                key: "{label}",
                                class: "palette-row",
                                class: if rank == palette_highlighted() { "selected" },
                                onclick: {
                                    let id = command.id;
                                    move |_| run_command.call((frozen, id))
                                },
                                span { class: "palette-label", "{label}" }
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
                                creator_query.set(typed.call((creator_query, event.value())));
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
                                            shown.write().wrote("");
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
                                                shown.write().wrote("");
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
                                        shown.write().wrote("");
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
        // the notices overlay floats in the palette's box on either screen:
        // the history newest first — a notice leaves the line by gesture or
        // resolution, never the record
        // (adr/2026-08-status-surface-owns-notices.md)
        if notices_open() {
            div {
                class: "command-palette",
                // the pane's own focus and keydown, like every other
                // overlay's input — a click or an Escape must reach this
                // div itself, not whatever held focus before it opened
                // (adr/2026-08-palette-order-and-overlay-placement.md)
                tabindex: "0",
                onmounted: move |event| async move {
                    let _ = event.set_focus(true).await;
                },
                onkeydown: move |event: KeyboardEvent| {
                    if event.key() == Key::Escape {
                        notices_open.set(false);
                    }
                },
                onclick: move |_| notices_open.set(false),
                div { class: "palette-head type-label", "notices" }
                if status.read().history().is_empty() {
                    div { class: "picker-empty", "nothing to report" }
                }
                for (rank, line) in status
                    .read()
                    .history()
                    .iter()
                    .rev()
                    .map(|entry| entry.text.clone())
                    .enumerate()
                {
                    div { key: "{rank}", class: "loops-line", "{line}" }
                }
            }
        }
        // the open-loops overlay, the notices overlay's sibling: rendered at
        // the same top level so the ember and the palette's "open loops"
        // command both reach it from either screen
        // (adr/2026-08-palette-order-and-overlay-placement.md). Nothing
        // renders at zero loops — the ember's own idiom
        // (adr/2026-07-debt-counter-then-list.md).
        if loops_open() && !loops.read().is_empty() {
            div {
                // the palette's own floating box, reused rather than
                // repeated — the settings overlay's own class does the same
                class: "command-palette loops-list",
                tabindex: "0",
                onmounted: move |event| async move {
                    let _ = event.set_focus(true).await;
                },
                // the link picker's arrow/enter grammar over a list with no
                // query field to carry it
                // (adr/2026-09-loop-lines-open-their-notes.md)
                onkeydown: move |event: KeyboardEvent| {
                    let last = loops.read().len().saturating_sub(1);
                    match event.key() {
                        Key::Escape => loops_open.set(false),
                        Key::Enter => {
                            if let Some(line) = loops.read().get(loops_highlighted()) {
                                open_loop.call(line.path.clone());
                            }
                        }
                        Key::ArrowDown => {
                            loops_highlighted.set((loops_highlighted() + 1).min(last));
                        }
                        Key::ArrowUp => {
                            loops_highlighted.set(loops_highlighted().saturating_sub(1));
                        }
                        _ => {}
                    }
                },
                onclick: move |_| loops_open.set(false),
                div { class: "loops-head type-label", "open loops" }
                for (rank, line) in loops().into_iter().enumerate() {
                    div {
                        key: "{rank}",
                        class: "loops-line loops-line-open",
                        class: if rank == loops_highlighted() { "selected" },
                        onclick: {
                            let path = line.path.clone();
                            move |event: MouseEvent| {
                                // a row click must not also reach the
                                // container's own onclick, which closes the
                                // overlay instead of opening the note
                                event.stop_propagation();
                                open_loop.call(path.clone());
                            }
                        },
                        "{line.text}"
                    }
                }
            }
        }
        // the settings overlay, the notices overlay's sibling — rendered at
        // the same top level so it stands over either screen
        // (adr/2026-08-settings-overlay.md)
        if settings_open() {
            div {
                class: "command-palette settings",
                tabindex: "0",
                onmounted: move |event| async move {
                    let _ = event.set_focus(true).await;
                },
                onkeydown: move |event: KeyboardEvent| {
                    if event.key() == Key::Escape {
                        settings_open.set(false);
                    }
                },
                div { class: "palette-head type-label", "settings" }
                div { class: "settings-row",
                    span { "theme" }
                    button {
                        r#type: "button",
                        title: "toggle theme",
                        onclick: move |_| root_commands.toggle_theme.call(()),
                        "{theme_label}"
                    }
                }
                div { class: "settings-row",
                    span { "font size" }
                    div { class: "settings-stepper",
                        button {
                            r#type: "button",
                            title: "decrease font size",
                            onclick: move |_| {
                                font_size
                                    .set(
                                        font_size()
                                            .saturating_sub(FONT_STEP)
                                            .max(MIN_FONT_SIZE),
                                    );
                            },
                            "−"
                        }
                        span { "{font_size()}px" }
                        button {
                            r#type: "button",
                            title: "increase font size",
                            onclick: move |_| {
                                font_size
                                    .set((font_size() + FONT_STEP).min(MAX_FONT_SIZE));
                            },
                            "+"
                        }
                    }
                }
            }
        }
        // the note switcher, the settings overlay's sibling: rendered at
        // the top level so it floats over every screen, in the pickers'
        // one grammar — query, arrows, Enter, Escape, clickable rows
        // (adr/2026-09-ctrl-o-is-the-one-note-switcher.md)
        {
            match switcher() {
                Some(frozen) => {
                    let empty_query = switcher_query.read().is_empty();
                    let rows: Vec<links::Completion> =
                        switcher_rows(&frozen, &switcher_query.read())
                            .into_iter()
                            .cloned()
                            .collect();
                    let keys_rows = rows.clone();
                    rsx! {
                    div { class: "command-palette",
                        div { class: "palette-head type-label", "open note" }
                        input {
                            class: "picker-query",
                            value: "{switcher_query}",
                            placeholder: "note…",
                            onmounted: move |event| async move {
                                let _ = event.set_focus(true).await;
                            },
                            oninput: move |event| {
                                switcher_query.set(typed.call((switcher_query, event.value())));
                                switcher_highlighted.set(0);
                            },
                            onkeydown: move |event: KeyboardEvent| {
                                let key = event.key();
                                let last = keys_rows.len().saturating_sub(1);
                                match key {
                                    Key::Escape => close_switcher.call(()),
                                    Key::Enter => {
                                        if let Some(entry) = keys_rows.get(switcher_highlighted()) {
                                            switch_to.call(entry.id.clone());
                                        }
                                    }
                                    Key::ArrowDown => {
                                        switcher_highlighted.set((switcher_highlighted() + 1).min(last));
                                    }
                                    Key::ArrowUp => {
                                        switcher_highlighted.set(switcher_highlighted().saturating_sub(1));
                                    }
                                    _ => {}
                                }
                                if !event.modifiers().ctrl() {
                                    event.stop_propagation();
                                }
                            },
                        }
                        // an empty list says which emptiness it is: a vault
                        // never navigated has no recent notes, a query can
                        // simply match none
                        if rows.is_empty() {
                            div { class: "picker-empty",
                                if empty_query { "no note visited yet" } else { "no matching note" }
                            }
                        }
                        for (rank, entry) in rows.into_iter().enumerate() {
                            div {
                                key: "{entry.id}",
                                class: "picker-row",
                                class: if rank == switcher_highlighted() { "selected" },
                                onclick: {
                                    let id = entry.id.clone();
                                    move |_| switch_to.call(id.clone())
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
        // the finder, the recent-notes picker's twin over the vault's
        // text: query, hits with their snippet, arrows, Enter, Escape
        // (adr/2026-09-full-text-search-lives-in-the-index.md)
        if finder_open() {
            {
                let rows = finder_hits();
                let keys_rows = rows.clone();
                rsx! {
                div { class: "command-palette",
                    div { class: "palette-head type-label", "search text" }
                    input {
                        class: "picker-query",
                        value: "{finder_query}",
                        placeholder: "words…",
                        onmounted: move |event| async move {
                            let _ = event.set_focus(true).await;
                        },
                        oninput: move |event| finder_typed.call(typed.call((finder_query, event.value()))),
                        onkeydown: move |event: KeyboardEvent| {
                            let key = event.key();
                            let last = keys_rows.len().saturating_sub(1);
                            match key {
                                Key::Escape => close_finder.call(()),
                                Key::Enter => {
                                    if let Some(hit) = keys_rows.get(finder_highlighted()) {
                                        close_finder.call(());
                                        open_loop.call(hit.path.clone());
                                    }
                                }
                                Key::ArrowDown => {
                                    finder_highlighted.set((finder_highlighted() + 1).min(last));
                                }
                                Key::ArrowUp => {
                                    finder_highlighted.set(finder_highlighted().saturating_sub(1));
                                }
                                _ => {}
                            }
                            if !event.modifiers().ctrl() {
                                event.stop_propagation();
                            }
                        },
                    }
                    if rows.is_empty() {
                        div { class: "picker-empty",
                            if finder_query.read().trim().is_empty() { "type to search the vault" } else { "no matching note" }
                        }
                    }
                    for (rank, hit) in rows.into_iter().enumerate() {
                        div {
                            key: "{hit.path.display()}",
                            class: "picker-row",
                            class: if rank == finder_highlighted() { "selected" },
                            onclick: {
                                let path = hit.path.clone();
                                move |_| {
                                    close_finder.call(());
                                    open_loop.call(path.clone());
                                }
                            },
                            span { class: "picker-id",
                                {hit.title.clone().unwrap_or_else(|| crate::domain::stem_of(&hit.path))}
                            }
                            span { class: "picker-title", "{hit.snippet}" }
                        }
                    }
                }
                }
            }
        }
        if screen() == Screen::Logs {
            div {
                class: "logs",
                // one CSS token, so the editor's textarea and every
                // rendered fragment inside stay the same size at every
                // window width (adr/2026-08-one-font-size-for-source-and-render.md)
                style: "{prose_size_style(font_size())}",
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
                nav {
                    class: "rail",
                    class: if rail_folded() { "folded" },
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
                  div { class: "centre-column",
                    div { class: "crumbs",
                        // an open template wears its own crumbs: it has no
                        // scale chain to climb
                        if let Some(name) = &template_open {
                            span { class: "crumb", "templates" }
                            span { class: "crumb", "{name}" }
                        } else {
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
                    }
                    {
                        match &notice {
                            Some(shown) => rsx! { p { class: "notice {shown.class()}", "{shown.text}" } },
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
                    {template_view()}
                    {search_view()}
                    {ex_view()}
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
                }
                aside {
                    class: "jump",
                    class: if jump_folded() { "folded" },
                    // months page by scrolling — no ‹ › buttons (adr/2026-07-month-paging-arrow-keys.md)
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
                                title: "previous month",
                                onclick: move |_| page.call(false),
                                "‹"
                            }
                            button {
                                class: "cal-today",
                                title: "open today's note",
                                onclick: move |_| open_daily.call(()),
                                "today"
                            }
                            button {
                                class: "cal-arrow",
                                title: "next month",
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
                // one CSS token, so the editor's textarea and every
                // rendered fragment inside stay the same size at every
                // window width (adr/2026-08-one-font-size-for-source-and-render.md);
                // the sheet nests inside this div, so it inherits the same
                // custom property
                style: "{prose_size_style(font_size())}",
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
                                        match card_body(&bodies, &feed, &root, &card.path, theme) {
                                            Ok(Some(svg)) => rsx! {
                                                div { class: "note", dangerous_inner_html: "{svg}" }
                                            },
                                            // nothing compiled yet, fresh or
                                            // stale: a quiet gap until the
                                            // SVG lands
                                            Ok(None) => rsx! {
                                                div { class: "body-pending" }
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
                        // the index card's whole frame, from the same
                        // function the tether measures against, so the line
                        // always meets the card it points at; the frame is
                        // already bounded by the observed pane, which is
                        // what a narrow window needs — no CSS ceiling on
                        // top of it (adr/2026-09-the-sheet-is-an-index-card.md)
                        style: {
                            let frame = table::sheet_frame(viewport());
                            format!(
                                "left: {}px; top: {}px; width: {}px; height: {}px",
                                frame.left, frame.top, frame.width, frame.height
                            )
                        },
                        // a press inside the sheet is the sheet's own (text
                        // selection, block clicks) — never the void's pan
                        onmousedown: move |event: MouseEvent| event.stop_propagation(),
                        {
                            match &notice {
                                Some(shown) => rsx! { p { class: "notice {shown.class()}", "{shown.text}" } },
                                None => rsx! {},
                            }
                        }
                        div { class: "sheet-column",
                            {blocks_view().unwrap_or_else(|| rsx! {})}
                        }
                        {picker_view()}
                        {
                            // backlinks only, as a count ("← 2") — absent at
                            // zero, the ember's idiom (adr/2026-08-shipped-ui-is-the-spec.md)
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
                // the edit-template picker, the same command the logs
                // answer: it floats in the palette's box but lives inside
                // this branch like the finders below, so its chords bubble
                // to the table pane the way they bubble to the logs pane
                // (adr/2026-09-edit-template-reaches-the-logs-from-the-table.md)
                {template_view()}
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
                                    value: "{filter_query}",
                                    placeholder: "tag or type…",
                                    onmounted: move |event| async move {
                                        let _ = event.set_focus(true).await;
                                    },
                                    oninput: move |event| {
                                        filter_query.set(typed.call((filter_query, event.value())));
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
            }
        }
    }
}

/// The `.logs` pane's `--prose-size` override: one custom property, carrying
/// the current signal down to `.block-active` and its siblings, so the
/// editor's textarea and every rendered fragment inside read the same
/// number (adr/2026-08-one-font-size-for-source-and-render.md).
fn prose_size_style(size: u16) -> String {
    format!("--prose-size: {size}px")
}

/// What one `WalkVisual` act asks for, kept together so the walk's own
/// signature stays readable. `held` is the run's goal column as it stood
/// the moment the key landed, read here rather than inside the spawned
/// walk: a key pressed before that walk is first polled would otherwise
/// have already forgotten the run, and the walk could not tell.
#[derive(Clone, Copy)]
struct Step {
    down: bool,
    count: usize,
    extend: bool,
    held: Goal,
}

/// One `[count]j`/`k` press, resolved off the UI thread: the seam walks
/// every step inside the webview and answers one landing, and the logical
/// fallback takes the run's last step whenever the drawn lines ran out
/// first — which is what crosses into a neighbouring block and clamps at
/// the note's ends
/// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
async fn walk_visual(
    mut editor: Signal<Editor>,
    probe: Option<LineProbe>,
    goal: Rc<Cell<Goal>>,
    fragments: Rc<RefCell<FragmentCache>>,
    step: Step,
) {
    let held = step.held;
    let landing = match probe {
        // no seam injected at all: the run is wholly degraded and takes
        // its whole count logically, exactly as separate presses holding
        // the same goal column would. `None` out of the seam reads the
        // same way: a miss, the caret already on the note's first or last
        // drawn line, or a landing outside the active block
        None => None,
        Some(probe) => (probe.0)(held.x, step.down, step.count).await,
    };
    // another key has forgotten this run while the probe was out, so it
    // has already moved the caret this walk was about to place: the whole
    // walk is dropped — its landing, its goal column and its fallback
    if goal.get().generation != held.generation {
        return;
    }
    let before = editor.peek().active();
    let taken = landing.map_or(0, |landing| {
        remember(&goal, |run| run.x = Some(landing.x));
        editor.write().place_in_block(
            landing.start,
            landing.units,
            step.extend,
        );
        landing.taken.min(step.count)
    });
    // the drawn lines ran out before the count did: the rest of the run
    // goes logically, which is what leaves the active block
    if taken < step.count {
        let rest = step.count - taken;
        let column = walk_visual_fallback(editor, step, rest, held.column);
        remember(&goal, |run| run.column = column);
    }
    // a landing that woke another block leaves a stale fragment behind
    if editor.peek().active() != before {
        fragments.borrow_mut().sweep();
    }
}

/// Stores what a step resolved into the run's goal column, read back
/// rather than written from the walk's own copy: consecutive presses of
/// the same run share a generation, so the earlier step's column is
/// already there.
fn remember(goal: &Cell<Goal>, store: impl FnOnce(&mut Goal)) {
    let mut run = goal.get();
    store(&mut run);
    goal.set(run);
}

/// An explicit bound on one run before anything walks: a note draws at
/// most one visual line per character, so a count past that can never
/// reach further than its last drawn line, and `999999999j` asks the
/// webview for a walk it can finish (CLAUDE.md: bounded loops with
/// explicit iteration limits). No note open, nothing to walk: one step.
/// Where a freshly mounted caret puts itself in the pane, and the one rule
/// that keeps it honest: the anchor is **consumed** by the mount that uses
/// it and falls back to `Nearest`. The caret also remounts whenever an
/// async fragment compile lands, and a latched `Center` would scroll the
/// note out from under a reader who pressed nothing at all — the interface
/// moves only when the user moved it (AIR LAY-2,
/// adr/2026-08-scroll-anchor-is-consumed-once.md).
async fn settle_caret(
    event: Event<MountedData>,
    mut anchor: Signal<(vim::Anchor, u32)>,
) {
    let (wanted, nonce) = anchor();
    if wanted != vim::Anchor::Nearest {
        anchor.set((vim::Anchor::Nearest, nonce));
    }
    let vertical = match wanted {
        vim::Anchor::Nearest => ScrollLogicalPosition::Nearest,
        vim::Anchor::Center => ScrollLogicalPosition::Center,
        vim::Anchor::Top => ScrollLogicalPosition::Start,
        vim::Anchor::Bottom => ScrollLogicalPosition::End,
    };
    let _ = event
        .scroll_to_with_options(ScrollToOptions {
            behavior: ScrollBehavior::Instant,
            vertical,
            // the pane never scrolls sideways on its own
            horizontal: ScrollLogicalPosition::Nearest,
        })
        .await;
}

fn bounded_steps(editor: &Editor, count: usize) -> usize {
    editor
        .note()
        .map_or(1, |(_, text)| count.min(text.chars().count() + 1))
}

/// The degraded path a `WalkVisual` run falls back to whenever the line
/// probe has nothing left to answer: the same logical-line walk
/// `run_motion` always did, which is what crosses into a neighbouring
/// block and clamps at the note's ends. Answers the cluster column the run
/// must keep, so a fallback step over a short line does not forget where
/// the run started
/// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
/// `steps` is what is left of the run, not `step.count`: the seam may
/// already have walked some of it.
fn walk_visual_fallback(
    mut editor: Signal<Editor>,
    step: Step,
    steps: usize,
    column: Option<usize>,
) -> Option<usize> {
    let landed = {
        let snapshot = editor.peek();
        let text = snapshot.note().map(|(_, text)| text);
        text.zip(snapshot.caret()).and_then(|(text, caret)| {
            let lines = Lines::of(text, snapshot.blocks());
            let motion = if step.down { Motion::Down } else { Motion::Up };
            motions::motion(
                text, &lines, caret.head, motion, steps, column, None,
            )
        })
    };
    // note()/caret() are None only when no note is open — every real
    // caller only reaches here from a keystroke the sink already gated on
    // an open note with a caret; the branch below is proven by a direct
    // call in the tests, the one state a keystroke can never produce.
    landed.and_then(|(target, goal)| {
        if step.extend {
            editor.write().extend_to(target);
        } else {
            editor.write().place_at(target);
        }
        goal
    })
}

/// The app's two screens (adr/2026-07-two-screens-table-and-logs.md): the
/// table mounts as of v1 phase 2.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Table,
    Logs,
}

/// One entry in the visit log: what was showing right before it was left,
/// either a logs selection or an open sheet
/// (adr/2026-08-note-history-back.md).
#[derive(Clone, PartialEq, Debug)]
enum Visit {
    Logs(Selection),
    Sheet(String),
}

impl Visit {
    /// The note id the switcher's row shows: a visit is a note either way
    /// (adr/2026-09-ctrl-o-is-the-one-note-switcher.md).
    fn id(&self) -> &str {
        match self {
            Visit::Logs((_, id)) => id,
            Visit::Sheet(id) => id,
        }
    }
}

/// The visit log's cap: a bounded log, not an unbounded one
/// (adr/2026-08-note-history-back.md).
const HISTORY_CAP: usize = 64;

/// Pushes a visit, dropping the oldest once the cap is reached.
fn push_visit(history: &mut Vec<Visit>, visit: Visit) {
    history.push(visit);
    if history.len() > HISTORY_CAP {
        history.remove(0);
    }
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

/// The one-line chrome (adr/2026-08-shipped-ui-is-the-spec.md): two 14×14 stroked icons, the
/// current screen's lit and each a button to its screen
/// (adr/2026-08-screen-switch-gesture.md), the open-loops ember, and the
/// liveness glyph. Zero loops renders nothing at all — absence, not a zero
/// — so the ember is clickable exactly when there is a list to show
/// (adr/2026-08-loops-list-overlay.md). The glyph is the one thing that
/// never disappears: liveness is a fact in every state, rendered in the
/// same place — a ring watching, filled otherwise, so the state survives
/// greyscale (adr/2026-08-status-surface-owns-notices.md).
#[component]
fn Chrome(
    screen: Screen,
    loops: usize,
    filter: Option<String>,
    liveness: Liveness,
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
                title { "table view" }
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
                title { "logs view" }
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
                    title: "open loops",
                    onclick: move |_| on_ember.call(()),
                    "{loops}"
                }
            }
            svg {
                class: "liveness {liveness.class()}",
                width: "14",
                height: "14",
                view_box: "0 0 14 14",
                circle {
                    cx: "7",
                    cy: "7",
                    r: "3",
                    fill: if liveness.filled() { "currentColor" } else { "none" },
                    stroke: "currentColor",
                }
            }
        }
    }
}

/// The selected note's editor: opened when the index says the note exists,
/// closed otherwise — selection ≠ existence, so an empty selection must not
/// touch the filesystem.
fn open_selected(root: &Path, exists: bool, id: &str) -> Editor {
    let path = time_note_path(root, id);
    // `exists` names the rail's knowledge, which is the index's — but the
    // file is the truth. A typeless or id-less time note is loops debt
    // the rail excludes, yet its file opens fine
    // (adr/2026-09-loop-lines-open-their-notes.md)
    if exists || path.is_file() {
        Editor::open(path)
    } else {
        Editor::closed()
    }
}

/// Turns the native boundary's explicit result into the editor's optional
/// text and keeps the status source aligned with the latest read outcome.
fn clipboard_answer(
    answer: Result<String, String>,
    mut status: Signal<Status>,
) -> Option<String> {
    match answer {
        Ok(text) => {
            status.write().resolve(Source::Clipboard);
            Some(text)
        }
        Err(detail) => {
            status.write().report(Notice::clipboard_failed(&detail));
            None
        }
    }
}

/// The open template's name, when the one editor holds a file under
/// `templates/` — the whole "template mode", derived from the buffer's own
/// path rather than tracked beside it
/// (adr/2026-08-template-editing-in-the-one-editor.md).
fn open_template(editor: &Editor, root: &Path) -> Option<String> {
    let (path, _) = editor.note()?;
    let relative = path.strip_prefix(root.join("templates")).ok()?;
    Some(
        relative
            .to_string_lossy()
            .trim_end_matches(".typ")
            .to_string(),
    )
}

/// A time note's id is its stem, so the path needs no index round-trip.
fn time_note_path(root: &Path, id: &str) -> PathBuf {
    root.join(NoteCategory::Time.as_dir())
        .join(format!("{id}.typ"))
}

/// The relative form every `VaultChange` carries: the app's own write seams
/// (create, capture, delete) call this on the absolute path `create` /
/// `template` hand back before submitting a `compute::touched` or
/// `compute::removed` job, so the index catches up inside the same tick
/// instead of waiting on the watcher to notice the app's own write
/// (adr/2026-09-the-app-indexes-its-own-writes.md). `path` is always under
/// `root` by construction, so the fallback never fires outside a test that
/// deliberately passes something else.
fn vault_relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// The category a vault-relative path's leading directory names —
/// `NoteCategory::from_dir`'s verdict, the one authority every reader
/// consults (`src/domain.rs`), so a restored or loop-opened time note is
/// never mistaken for a permanent one. The Permanent fallback is
/// defensive only: every caller hands a path recorded under one of the
/// four category directories.
fn dir_category(relative: &Path) -> NoteCategory {
    relative
        .iter()
        .next()
        .and_then(|dir| dir.to_str())
        .and_then(NoteCategory::from_dir)
        .unwrap_or(NoteCategory::Permanent)
}

/// What a paste puts into the note: the clipboard's text when it holds
/// any; otherwise its image, written into `assets/` under the note's stem
/// and a timestamp and spelled as the `#image` call that shows it; nothing
/// when it holds neither, when the note has no name to file the image
/// under, or when a read or the write refused — each refusal a notice
/// (adr/2026-09-an-image-pastes-into-assets.md).
async fn pasted(
    clipboard: Clipboard,
    image: Option<ClipboardImage>,
    now: Option<Now>,
    root: PathBuf,
    stem: Option<String>,
    mut status: Signal<Status>,
) -> Option<String> {
    // a clipboard holding only an image refuses the text read outright
    // on X11, so a refusal is not the end: it is reported only once the
    // image read has found nothing either
    let refused = match (clipboard.0)().await {
        Ok(clip) if !clip.is_empty() => {
            status.write().resolve(Source::Clipboard);
            return Some(clip);
        }
        Ok(_) => None,
        Err(detail) => Some(detail),
    };
    let Some(((image, now), stem)) = image.zip(now).zip(stem) else {
        return refused.and_then(|detail| {
            status.write().report(Notice::clipboard_failed(&detail));
            None
        });
    };
    let png = match (image.0)().await {
        Ok(Some(png)) => png,
        Ok(None) => {
            return refused.and_then(|detail| {
                status.write().report(Notice::clipboard_failed(&detail));
                None
            });
        }
        Err(detail) => {
            status.write().report(Notice::clipboard_failed(&detail));
            return None;
        }
    };
    let name = image_asset_name(&stem, &(now.0)());
    match crate::persist::write_atomic_bytes(
        &root.join("assets").join(&name),
        &png,
    ) {
        Ok(_) => {
            status.write().resolve(Source::Clipboard);
            Some(format!("#image(\"/assets/{name}\")"))
        }
        Err(error) => {
            status
                .write()
                .report(Notice::image_failed(&error.to_string()));
            None
        }
    }
}

/// `assets/<stem>-<yyyymmdd-hhmmss>.png`: the note it was pasted into and
/// the moment, so two pastes never collide and a directory listing reads
/// as a timeline.
fn image_asset_name(stem: &str, now: &jiff::Zoned) -> String {
    format!("{stem}-{}.png", now.strftime("%Y%m%d-%H%M%S"))
}

/// The finder's hits for a query, read from the index per keystroke — the
/// `completions` pattern; a read that fails is the caller's to report
/// (adr/2026-09-full-text-search-lives-in-the-index.md).
fn search_hits(
    root: &Path,
    query: &str,
) -> Result<Vec<crate::index::SearchHit>, String> {
    let index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("search: {err:?}"))?;
    index
        .search(query)
        .map_err(|err| format!("search: {err:?}"))
}

/// The sheet's editor: the card knows its id, not its file, so the path is
/// looked up per event — the `completions` pattern. A lookup that fails is
/// the caller's to report: the error goes to the status surface, never
/// onto the editor (adr/2026-08-status-surface-owns-notices.md).
fn open_sheet_note(root: &Path, id: &str) -> Result<Editor, String> {
    let index = Index::open(&root.join(".index/index.db"))
        .map_err(|err| format!("sheet: {err:?}"))?;
    let path = index
        .path_for_id(&crate::domain::NoteId(id.to_string()))
        .map_err(|err| format!("sheet: {err:?}"))?;
    match path {
        Some(path) => Ok(Editor::open(root.join(path))),
        None => Err(format!("sheet: no note has the id {id}")),
    }
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

/// One centre-pane slot, one per block of the note, each tagged with its
/// own block's `start` byte so a click activates the block actually
/// clicked rather than a fixed boundary neighbour
/// (adr/2026-08-css-draws-the-markup.md, superseding the cursor split's
/// merged-region compromise).
enum Pane {
    Source {
        start: usize,
        text: String,
    },
    /// A block the note-global visual selection reaches into, drawn as
    /// highlighted raw source rather than through the markup model — the
    /// active block keeps the widget, this one keeps only the pixels
    /// (adr/2026-08-visual-selection-drawn-across-lines.md).
    Selected {
        start: usize,
        lines: Vec<caret::Line>,
        /// The block's own content, so the pane can run it through
        /// `markup::model` the same way an active or an inactive block
        /// does — `lines` alone carries no source text to derive a
        /// verdict from (adr/2026-08-css-draws-the-markup.md).
        text: String,
    },
    /// A block CSS can draw: its structural role and the spans tiling its
    /// content, laid out as styled `<span>`s rather than a compiled SVG
    /// (adr/2026-08-css-draws-the-markup.md).
    Css {
        start: usize,
        block: markup::BlockRole,
        spans: Vec<markup::Span>,
        text: String,
    },
    Fragment {
        start: usize,
        rendered: Result<String, String>,
    },
    Pending {
        start: usize,
        text: String,
        job: Option<crate::render::FragmentJob>,
        /// The slot's last SVG, standing in while the new compile is out
        /// (adr/2026-09-fragments-shelve-their-last-svg-per-block.md).
        shelved: Option<String>,
    },
}

/// Splits the note into one pane per block: the active block stays raw
/// source, every other block the visual selection reaches into draws as
/// highlighted raw source, and every remaining block draws from the markup
/// model — styled CSS spans when it can, a cached compiled-SVG widget when
/// it can't (adr/2026-08-css-draws-the-markup.md,
/// adr/2026-08-visual-selection-drawn-across-lines.md).
fn block_panes(
    editor: &Editor,
    root: &Path,
    theme: RenderTheme,
    cache: &mut FragmentCache,
    queued: bool,
    linewise: bool,
) -> Option<Vec<Pane>> {
    let (file, text) = editor.note()?;
    let blocks = editor.blocks();
    // `blocks::segment` never returns an empty vec, and every mutator
    // (`activate`, `restore`, `resize`) keeps `active` in bounds for it —
    // clamping rather than a second fallible lookup avoids a branch this
    // invariant makes unreachable in practice (the std `min` call carries
    // no region of its own to leave uncovered)
    let active_index = editor.active()?.min(blocks.len().saturating_sub(1));
    let active = &blocks[active_index];
    // widened to whole lines under `V`, the same rule `visual_span` cuts an
    // operator's span with (vim.rs) — otherwise the covered boundary block
    // would draw only the raw anchor..head intersection, a ragged partial
    // line where `d`/`y`/`c` already take the whole line
    // (adr/2026-08-visual-selection-drawn-across-lines.md)
    let selection = editor.selection().map(|sel| {
        if linewise {
            let lines = motions::Lines::of(text, blocks);
            let first = lines.row_of(sel.start);
            let last = lines.row_of(sel.end);
            motions::linewise_span(&lines, first, last)
        } else {
            sel
        }
    });
    // the selection's own reach above/below the active block: only past
    // that edge does a block need to split out into its own highlighted
    // pane instead of the markup model's rendering
    // (adr/2026-08-visual-selection-drawn-across-lines.md) — the boundary
    // block on each side is found once, up front, the same way the cursor
    // split did, rather than re-deriving it per block in the loop below
    let above_boundary = selection
        .as_ref()
        .filter(|sel| sel.start < active.range.start)
        .map(|sel| blocks::block_at(blocks, sel.start));
    let below_boundary = selection
        .as_ref()
        .filter(|sel| sel.end > active.range.end)
        .map(|sel| blocks::block_at(blocks, sel.end.saturating_sub(1)));
    let sel = selection.unwrap_or(0..0);

    let mut panes = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        if index == active_index {
            panes.push(Pane::Source {
                start: active.range.start,
                text: text.get(active.content()).unwrap_or("").to_string(),
            });
            continue;
        }
        let selected = above_boundary
            .is_some_and(|b| index >= b && index < active_index)
            || below_boundary
                .is_some_and(|b| index > active_index && index <= b);
        if selected {
            panes.push(selected_pane(text, block, &sel));
            continue;
        }
        panes.push(
            match markup::model(text.get(block.content()).unwrap_or("")) {
                markup::Draw::Css(markup) => Pane::Css {
                    start: block.range.start,
                    block: markup.block,
                    spans: markup.spans,
                    text: text.get(block.content()).unwrap_or("").to_string(),
                },
                markup::Draw::Typst => block_pane(
                    text, block, index, file, root, theme, cache, queued,
                ),
            },
        );
    }
    Some(panes)
}

/// One block the selection covers but the widget does not: the note-global
/// selection intersected with the block's own content, translated to the
/// block's own bytes and split at its internal line breaks — every block
/// but the rare multi-line construct is exactly one line, so this is
/// usually one `Line` long. The intersection is provably non-inverted for
/// every block `block_panes` calls this on (`above`/`below` only ever name
/// blocks the selection's own reach already covers), so the block-relative
/// clamp here is a formality that also protects a future caller that is
/// not (adr/2026-08-visual-selection-drawn-across-lines.md).
fn selected_pane(
    text: &str,
    block: &blocks::Block,
    selection: &Range<usize>,
) -> Pane {
    let content = block.content();
    let start = content.start.max(selection.start).min(content.end);
    let end = content.end.min(selection.end).max(content.start);
    let local = (start - content.start)..(end - content.start);
    let source = text.get(content.clone()).unwrap_or("");

    let mut lines = Vec::new();
    let mut line_start = 0;
    for slice in source.split('\n') {
        lines.push(caret::Line {
            pieces: caret::layout_selected(slice, line_start, local.clone()),
        });
        line_start += slice.len() + 1;
    }
    Pane::Selected {
        start: content.start,
        lines,
        text: source.to_string(),
    }
}

/// One `Selected` pane's own piece, reduced to whether it's highlighted:
/// `caret::layout_selected` only ever emits `Text`/`Selected`, the two the
/// rsx below draws, but the match stays exhaustive over every `Piece`
/// rather than assuming that blind — a caret, its box, or a composition
/// preview draws as plain unhighlighted text, proven dead by a direct test
/// rather than trusted never to arrive
/// (adr/2026-08-visual-selection-drawn-across-lines.md).
fn piece_span(piece: caret::Piece) -> (bool, usize, String) {
    match piece {
        caret::Piece::Selected { start, text } => (true, start, text),
        caret::Piece::Text { start, text } => (false, start, text),
        caret::Piece::Caret => (false, 0, String::new()),
        caret::Piece::CaretBox { start, cluster } => (false, start, cluster),
        caret::Piece::Preview { start, text } => (false, start, text),
    }
}

/// A `Css` pane's own lines: one entry per physical line of the block's
/// content, each already split at every markup span boundary it crosses —
/// `markup::tint` does the splitting, fed one whole-line `Piece::Text` at a
/// time since a `Css` pane draws no caret, selection or IME preview of its
/// own (adr/2026-08-css-draws-the-markup.md).
fn markup_lines(
    text: &str,
    spans: &[markup::Span],
) -> Vec<Vec<(markup::Role, bool, caret::Piece)>> {
    let mut line_start = 0;
    text.split('\n')
        .map(|slice| {
            let piece = caret::Piece::Text {
                start: line_start,
                text: slice.to_string(),
            };
            line_start += slice.len() + 1;
            markup::tint(spans, vec![piece])
        })
        .collect()
}

/// One `Css` pane's own piece, reduced to its byte start and text —
/// `markup::tint` only ever answers `Piece::Text` for the `Piece::Text`
/// input `markup_lines` feeds it (the variant its `make` closure was given
/// carries straight through), but the match stays exhaustive over every
/// `Piece` rather than assuming that blind, the same call `piece_span`
/// already makes for `Pane::Selected`.
fn css_piece_span(piece: caret::Piece) -> (usize, String) {
    match piece {
        caret::Piece::Text { start, text }
        | caret::Piece::Selected { start, text }
        | caret::Piece::Preview { start, text } => (start, text),
        caret::Piece::CaretBox { start, cluster } => (start, cluster),
        caret::Piece::Caret => (0, String::new()),
    }
}

/// One block's compiled-fallback pane: the block's own byte offset is what
/// a click on it activates, so a click on a fallback block always lands on
/// the block that was actually clicked
/// (adr/2026-08-css-draws-the-markup.md, superseding the cursor split's
/// boundary-block click compromise).
#[allow(clippy::too_many_arguments)]
fn block_pane(
    text: &str,
    block: &blocks::Block,
    slot: usize,
    file: &Path,
    root: &Path,
    theme: RenderTheme,
    cache: &mut FragmentCache,
    queued: bool,
) -> Pane {
    let source = blocks::block_source(text, block);
    let start = block.range.start;
    if queued {
        // the threaded adapter: never compile in the frame — probe, and
        // hand a miss's job up for the tier
        // (adr/2026-08-compute-tier-worker-seam.md); the slot is what a
        // changed block's last image is shelved under
        // (adr/2026-09-fragments-shelve-their-last-svg-per-block.md)
        match cache.probe(root, file, slot, &source, theme) {
            FragmentView::Ready(rendered) => {
                Pane::Fragment { start, rendered }
            }
            FragmentView::Pending { job, shelved } => Pane::Pending {
                start,
                text: text.get(block.content()).unwrap_or("").to_string(),
                job,
                shelved,
            },
        }
    } else {
        Pane::Fragment {
            start,
            rendered: cache.render(root, file, &source, theme),
        }
    }
}

/// One card's body at the Bodies zoom: the SVG to show (fresh, or stale
/// while its recompile is out), nothing yet, or the compile's error. A
/// miss queues the compile — through the tier when queued, in place when
/// inline (adr/2026-08-async-caches-pending-stale.md).
fn card_body(
    bodies: &Rc<RefCell<BodyCache>>,
    feed: &ComputeFeed,
    root: &Path,
    note: &Path,
    theme: RenderTheme,
) -> Result<Option<String>, String> {
    if feed.inline {
        return bodies.borrow_mut().render(root, note, theme).map(Some);
    }
    match bodies.borrow_mut().probe(root, note, theme) {
        BodyView::Ready(result) => result.map(Some),
        BodyView::Pending { stale, job } => {
            if let Some(job) = job {
                (feed.submit)(Job::Body(job));
            }
            Ok(stale)
        }
    }
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
    /// Whether the editor held a note: the export names it
    /// (adr/2026-09-export-writes-the-pdf-beside-the-note.md).
    note_open: bool,
    /// Which screen the palette opened over: the screen commands hide where
    /// they already stand (adr/2026-08-screen-switch-gesture.md).
    on_table: bool,
    /// Whether a sheet was open: delete exists only over one
    /// (adr/2026-08-delete-note-palette-only-from-sheet.md).
    sheet_open: bool,
    /// Whether the table stood at body zoom: each zoom command hides at its
    /// own level (adr/2026-08-body-zoom-scale-and-metrics.md).
    at_bodies: bool,
    /// Whether a save stood refused over an external edit: the resolution
    /// pair exists only while there is a side to pick
    /// (adr/2026-08-external-edit-conflict-commands.md).
    conflict: bool,
    /// Whether the undo register held anything: an empty register hides
    /// the command (adr/2026-08-app-level-undo-register.md).
    undoable: bool,
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

/// The open switcher's fixed half — the `Picker` idiom over two lists,
/// both frozen at open and both without the note currently showing:
/// `recent` is where you have been, `entries` everywhere you could go
/// (adr/2026-09-ctrl-o-is-the-one-note-switcher.md).
#[derive(Clone, PartialEq)]
struct Switcher {
    recent: Vec<links::Completion>,
    entries: Vec<links::Completion>,
}

/// What the switcher lists: with no query, the visit log — a switcher
/// opened and dismissed with Enter is a back button, which is what the
/// picker it replaces was for. With one, every note in the vault, matched
/// by id and title and capped exactly as the Ctrl+L picker matches, so
/// one rule covers both places a note is named
/// (adr/2026-09-ctrl-o-is-the-one-note-switcher.md).
fn switcher_rows<'a>(
    frozen: &'a Switcher,
    query: &str,
) -> Vec<&'a links::Completion> {
    if query.is_empty() {
        return frozen.recent.iter().collect();
    }
    links::filter(&frozen.entries, query)
}

/// The visit log read as a list of notes: each distinct id, newest visit
/// first, `own` — the note currently showing — left out. A visit is a note
/// either way, so the log's two surfaces fold into one row per note rather
/// than one per surface. The title is the index's own, when the vault
/// still holds a row for that id: a note visited and since deleted keeps
/// its row here, because the log is app state and answers with no read.
fn recent_notes(
    history: &[Visit],
    own: &str,
    entries: &[links::Completion],
) -> Vec<links::Completion> {
    let mut rows: Vec<links::Completion> = Vec::new();
    for visit in history.iter().rev() {
        let id = visit.id();
        if id == own || rows.iter().any(|row| row.id == id) {
            continue;
        }
        rows.push(match entries.iter().find(|entry| entry.id == id) {
            Some(entry) => entry.clone(),
            None => links::Completion {
                id: id.to_string(),
                title: None,
            },
        });
    }
    rows
}

/// The edit-template overlay's frozen half: the directory listing at the
/// moment it opened (adr/2026-08-template-editing-in-the-one-editor.md).
#[derive(Clone, PartialEq)]
struct TemplatePicker {
    entries: Vec<String>,
}

/// Every template the picker can offer, listed at the moment it opens:
/// templates are never in the index (`scan_vault` walks only the category
/// dirs), so the directory itself is the authority.
fn template_names(root: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(root.join("templates"))
        .map_err(|err| format!("templates: {err}"))?;
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".typ")
                .map(str::to_string)
        })
        .collect();
    names.sort();
    Ok(names)
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
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::atomic::Ordering;

    use dioxus::dioxus_core::{ElementId, Event, Mutation, Mutations};
    use dioxus::html::*;
    use dioxus::prelude::VirtualDom;

    use super::*;
    use crate::compute::{open_loops, survey};
    use crate::index::IndexError;

    /// Only a typst-rendered note carries the SVG namespace — the chrome's
    /// rsx icons don't — so this is the "a note is rendered" marker.
    const RENDERED_NOTE: &str = r#"xmlns="http://www.w3.org/2000/svg""#;

    /// The tests' clock: a Thursday inside the fixture week, so the initial
    /// selection is `time/2026-07-23.typ` and the grid opens on july 2026.
    const TODAY: &str = "2026-07-23";

    /// The same clock as a date, for the survey legs the tests call by hand.
    fn test_today() -> Date {
        TODAY.parse().expect("the test clock is a valid date")
    }

    /// Initial click-listener layout, established empirically (see the
    /// mounted-app doc): registration runs the chrome's two icons first,
    /// then jump-panel — the header's ‹ today › buttons, the three seasons,
    /// then each grid row as gutter + day cells — then the note's two
    /// link-footer entries, one click target per block above the active
    /// trailing empty line (adr/2026-08-css-draws-the-markup.md), the
    /// two crumb jumps, and finally the five rail rows top to bottom.
    const CHROME_TABLE: usize = 0;
    const CHROME_LOGS: usize = 1;
    const CAL_BACK: usize = 2;
    const CAL_TODAY: usize = 3;
    const CAL_FORWARD: usize = 4;
    const SEASON_AUTUMN: usize = 7;
    const GUTTER_W31: usize = 38;
    const FOOTER_BACKLINK: usize = 44;
    const FOOTER_OUTGOING: usize = 45;
    /// The fixture day note's own preamble block — a click always wakes
    /// exactly the block that was clicked now, so each block above the
    /// active trailing line gets its own listener instead of one merged
    /// region's (adr/2026-08-css-draws-the-markup.md).
    const BLOCK_PREAMBLE: usize = 46;
    const BLOCK_BLANK: usize = 47;
    const BLOCK_HEADING: usize = 48;
    const BLOCK_LINK: usize = 49;
    const CRUMB_WEEK: usize = 50;
    const RAIL_SUMMER: usize = 52;
    const RAIL_W30: usize = 53;
    const RAIL_DAY_23: usize = 54;
    const RAIL_DAY_22: usize = 55;
    const RAIL_DAY_21: usize = 56;
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
    fn an_unbuildable_index_degrades_instead_of_dying() {
        // the .typ files may be fine even when the index is not, so a
        // failed launch survey is a notice and a degraded glyph, never the
        // vault-error takeover (adr/2026-08-startup-survey-async.md)
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let (dom, _, _, _) = rendered_app(Some(dir.path().join("missing")));
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("vault-error"), "{html}");
        assert!(html.contains("indexing the vault"), "{html}");
        assert!(html.contains("liveness-degraded"), "{html}");
    }

    // -- the theme: one palette row, one attribute ---------------------------

    #[test]
    fn ctrl_t_no_longer_toggles_the_theme() {
        // ctrl+t went to the todo toggle
        // (adr/2026-08-ctrl-t-toggles-the-todo.md); the theme now changes
        // only through its palette row
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
        assert!(html.contains(r#"data-theme="dark""#), "unchanged: {html}");
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

    // -- the prose size: one token for the editor and the render ------------
    // (adr/2026-08-one-font-size-for-source-and-render.md)

    #[test]
    fn the_logs_pane_carries_the_default_prose_size() {
        let vault = temp_vault();
        let (dom, ..) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"style="--prose-size: 18px""#), "{html}");
    }

    #[test]
    fn prose_size_style_reflects_a_changed_signal() {
        // the settings-page stepper is what drives `font_size` in the UI;
        // this pins the format its value reaches the style attribute
        // through, so a signal change is visible the moment it lands
        assert_eq!(prose_size_style(18), "--prose-size: 18px");
        assert_eq!(prose_size_style(24), "--prose-size: 24px");
    }

    // -- the todo toggle: Ctrl+T flips the caret's line's checkbox ----------
    // (adr/2026-08-ctrl-t-toggles-the-todo.md)

    #[test]
    fn ctrl_t_turns_the_caret_line_into_an_unchecked_item_on_the_logs_screen()
    {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );

        assert!(source_of(&dom).ends_with("- [ ] "), "{}", source_of(&dom));
    }

    #[test]
    fn a_second_ctrl_t_checks_the_item_off_on_the_logs_screen() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );

        assert!(source_of(&dom).ends_with("- [x] "), "{}", source_of(&dom));
    }

    #[test]
    fn ctrl_t_flips_the_checkbox_on_the_table_sheet_too() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, sink) = sheet_block_targets(&opened);

        press(
            &mut dom,
            sink,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );

        assert!(source_of(&dom).ends_with("- [ ] "), "{}", source_of(&dom));
    }

    #[test]
    fn ctrl_t_does_nothing_without_an_active_block_on_the_logs_screen() {
        // an empty day holds a closed editor: no block, nothing to toggle
        // (adr/2026-08-ctrl-t-toggles-the-todo.md)
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(20)]);
        let before = dioxus_ssr::render(&dom);

        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );

        let after = dioxus_ssr::render(&dom);
        assert_eq!(before, after, "nothing to toggle, nothing changed");
    }

    #[test]
    fn ctrl_t_does_nothing_without_an_active_block_on_the_table_screen() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[day_cell(20)]);
        // the bare table, no sheet open: the one editor still holds the
        // closed empty-day state
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        let before = dioxus_ssr::render(&dom);

        press(
            &mut dom,
            keys,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );

        let after = dioxus_ssr::render(&dom);
        assert_eq!(before, after, "nothing to toggle, nothing changed");
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
        // the overlay grabs its own focus on mount now (its own onmounted,
        // adr/2026-08-palette-order-and-overlay-placement.md) rather than
        // the shared pane-focus effect, which must stand aside while the
        // overlay is open or the two would race for the same focus
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let mutations = click_for_mutations(&mut dom, clicks[EMBER]);
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );
        assert!(
            focused.load(Ordering::SeqCst) > 0,
            "the loops list asks for focus as it mounts"
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

        lock_dir(&vault.path().join("time"), true);
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
        // the deposit is drained by the forwarding effect, one poll later
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("time"), false);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-critical"), "{html}");
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

        lock_dir(&vault.path().join("time"), true);
        // the settle loop spans several autosave restarts, so the
        // value-gated write is exercised on both of its sides here
        retype(&mut dom, sink, "= en panne\n");
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-critical"), "{html}");
        assert!(html.contains("2026-07-23.typ"), "{html}");

        // writable again: the save that lands resolves its own failure —
        // no gesture (adr/2026-08-status-surface-owns-notices.md)
        lock_dir(&vault.path().join("time"), false);
        retype(&mut dom, sink, "= réparé\n");
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("notice-critical"), "resolved: {html}");
    }

    // -- the external-edit conflict and its palette fork ---------------------
    // (adr/2026-08-external-edit-conflict-commands.md)

    #[test]
    fn an_external_edit_refuses_the_autosave_and_summons_the_fork() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        let file = vault.path().join("time/2026-07-23.typ");
        edit_behind(&file, "= repris dehors\n");
        retype(&mut dom, sink, "= à moi\n");
        block_on(settle(&mut dom));

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-critical"), "{html}");
        assert!(html.contains("changed on disk"), "{html}");
        assert_eq!(
            std::fs::read_to_string(&file).expect("the note is readable"),
            "= repris dehors\n",
            "the outside author was not clobbered"
        );

        // the fork exists exactly while the conflict stands
        open_palette(&mut dom, sink);
        let labels = palette_labels(&dom);
        assert!(labels.contains(&"keep mine".to_string()), "{labels:?}");
        assert!(labels.contains(&"take disk".to_string()), "{labels:?}");
    }

    #[test]
    fn keep_mine_overwrites_the_disk_and_resolves_the_conflict() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        let file = vault.path().join("time/2026-07-23.typ");
        edit_behind(&file, "= repris dehors\n");
        retype(&mut dom, sink, "= à moi\n");
        block_on(settle(&mut dom));

        let (input, palette_keys) = open_palette(&mut dom, sink);
        type_into(&mut dom, input, "keep mine");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));

        let saved =
            std::fs::read_to_string(&file).expect("the note is readable");
        assert!(saved.contains("= à moi"), "the buffer won: {saved}");
        assert!(!saved.contains("dehors"), "{saved}");
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("notice-"), "resolved: {html}");
    }

    #[test]
    fn a_keep_mine_the_disk_refuses_leaves_the_conflict_standing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        let file = vault.path().join("time/2026-07-23.typ");
        edit_behind(&file, "= repris dehors\n");
        retype(&mut dom, sink, "= à moi\n");
        block_on(settle(&mut dom));

        // the write itself fails now: picking a side resolved nothing
        lock_dir(&vault.path().join("time"), true);
        let (input, palette_keys) = open_palette(&mut dom, sink);
        type_into(&mut dom, input, "keep mine");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("time"), false);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-critical"), "{html}");
        assert_eq!(
            std::fs::read_to_string(&file).expect("the note is readable"),
            "= repris dehors\n",
            "nothing reached the disk"
        );
        // the fork still stands, because the conflict does
        open_palette(&mut dom, sink);
        let labels = palette_labels(&dom);
        assert!(labels.contains(&"keep mine".to_string()), "{labels:?}");
    }

    #[test]
    fn take_disk_reloads_the_buffer_and_resolves_the_conflict() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        let file = vault.path().join("time/2026-07-23.typ");
        edit_behind(&file, "= repris dehors\n");
        retype(&mut dom, sink, "= à moi\n");
        block_on(settle(&mut dom));

        let (input, palette_keys) = open_palette(&mut dom, sink);
        type_into(&mut dom, input, "take disk");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));

        let html = dioxus_ssr::render(&dom);
        // "repris dehors" is compiled into the static block's SVG now, not
        // literal text (adr/2026-08-per-line-block-segmentation.md); the
        // buffer's own unsaved edit going away is what "the disk won"
        // means here, confirmed on disk below
        assert!(!html.contains("à moi"), "the disk won: {html}");
        assert!(!html.contains("notice-"), "resolved: {html}");
        // the reloaded buffer re-armed the guard: its own autosave lands
        assert_eq!(
            std::fs::read_to_string(&file).expect("the note is readable"),
            "= repris dehors\n"
        );
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
                liveness: Liveness::Watching,
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
    fn the_chrome_icons_carry_hover_tooltips() {
        let mut dom = VirtualDom::new_with_props(
            BareChrome,
            BareChromeProps {
                screen: Screen::Table,
            },
        );
        dom.rebuild_to_vec();
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("<title>table view</title>"), "{html}");
        assert!(html.contains("<title>logs view</title>"), "{html}");
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
        assert!(
            html.contains(r#"class="ember" title="open loops">3</span>"#),
            "{html}"
        );
    }

    #[test]
    fn a_note_the_index_reads_dirty_joins_the_loops() {
        // the anomaly the index records is debt to see, not a silent
        // default (adr/2026-08-anomalies-join-the-loops.md)
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/bancal.typ"),
            "#meta(id: 42)\n\n= Bancal\n",
        )
        .expect("the malformed note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[EMBER]);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("bancal · malformed meta"),
            "the dirty read shows beside the typeless debt it caused: {html}"
        );
        assert!(html.contains("bancal · typeless"), "{html}");
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
    fn a_chrome_icon_switch_asks_the_pane_for_focus_even_before_it_remounts() {
        // regression: the shared focus effect never read `screen`, so a
        // mouse-driven switch with a block still active behind it
        // (adr/2026-08-cursor-always-in-the-note.md keeps it awake across
        // the switch) never re-ran the effect at all — only the freshly
        // mounted pane's own onmounted asked for focus, racing WebKitGTK's
        // native click-focus with nothing backing it up if that race was
        // lost (adr/2026-09-sheet-and-screen-join-the-focus-effect.md).
        // The logs pane's own handle is mounted and never replaced here —
        // the table pane's own mount is deliberately never delivered — so
        // any rise in the count can only be the effect reaching for
        // whatever the pane cell already holds, the instant the screen
        // changes and ahead of the new pane's own mount.
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let clicks = listeners(&mutations, "click");
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );
        block_on(settle(&mut dom));
        let before = focused.load(Ordering::SeqCst);

        click(&mut dom, clicks[CHROME_TABLE]);
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom).contains(r#"class="table""#),
            "the screen switched"
        );
        assert!(
            focused.load(Ordering::SeqCst) > before,
            "the effect re-asked for focus on the still-registered pane \
             handle the moment the screen changed"
        );
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
    fn enter_returns_to_the_note_on_the_block_that_held_the_caret() {
        // shift+Escape puts the note away; Enter picks it up again where
        // it was left — the preamble here, not the last block a fresh
        // open would wake (adr/2026-08-enter-returns-to-the-note.md)
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, block_keys) = activate_preamble(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "the preamble source: {html}");

        press(&mut dom, block_keys, Key::Escape, Modifiers::SHIFT);
        assert!(!dioxus_ssr::render(&dom).contains("block-active"));

        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "back in: {html}");
        assert!(html.contains("#import"), "on the same block: {html}");
        assert!(
            html.contains(r#"class="caret-box""#),
            "and still thinking: {html}"
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

        // the squat removed, the next landed write resolves the notice
        std::fs::remove_dir(vault.path().join(".index/positions"))
            .expect("the squat is removed");
        mouse(&mut dom, "mousedown", cards[0], (0.0, 0.0));
        mouse(&mut dom, "mousemove", pane, (60.0, 60.0));
        mouse(&mut dom, "mouseup", pane, (60.0, 60.0));
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("positions:"), "resolved: {html}");
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
        // alpha at (32, 32) stands under the centred index card: the tether
        // collapses to nothing rather than pointing through it
        // (adr/2026-09-the-sheet-is-an-index-card.md) — the tracking test
        // below drags the card out into the margin and reads a real line
        assert!(
            html.contains(
                r#"class="tether" style="left: 1090px; top: 60px; width: 0px""#
            ),
            "{html}"
        );
        // the card itself: 900 × 540, centred in the 1280 × 800 default
        assert!(
            html.contains(
                r#"style="left: 190px; top: 130px; width: 900px; height: 540px""#
            ),
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

    /// Plan item 8's second symptom, tested directly: opening the sheet on
    /// a multi-line note must render every non-active line, never leave one
    /// as raw `block-pending` source. Under the inline feed `rendered_app`
    /// gives every sheet test, this already passes — which says the
    /// symptom was the collapsing blank blocks fixed above, not a separate
    /// sheet/compute-tier wiring bug: the sheet and the logs pane share the
    /// one `blocks_view` closure and the one `compiled` signal it reads to
    /// wake on a landed compile. Rendering itself has since moved from one
    /// fragment per line to one region above the active line
    /// (adr/2026-08-cursor-split-rendering.md), so capture-idea's preamble,
    /// two headings and closing paragraph now compile as a single merged
    /// region rather than four separate fragments.
    #[test]
    fn a_sheet_renders_every_non_active_line_as_its_fragment() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, _keys) = table_targets_with_keys(&mut dom, &clicks);
        // capture-idea: preamble, two headings and the closing paragraph
        // are non-blank lines besides the active last (trailing) block
        open_sheet_on(&mut dom, pane, cards[1]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(RENDERED_NOTE), "{html}");
        assert!(!html.contains("block-pending"), "{html}");
        assert_eq!(
            html.matches(r#"class="note""#).count(),
            1,
            "every non-active line rendered together as one region: {html}"
        );
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
    fn plain_escape_leaves_the_sheet_open_and_shift_escape_closes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        // a plain Escape over the sheet's own chrome — no block focused —
        // never leaves the note (adr/2026-08-shift-escape-leaves-the-note.md)
        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains(r#"class="dim""#), "{html}");

        // only Shift+Escape takes the sheet with it
        press(&mut dom, keys, Key::Escape, Modifiers::SHIFT);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(!html.contains(r#"class="dim""#), "{html}");
        assert!(!html.contains("raised"), "{html}");
        // the card went back to its slot in the canvas
        assert!(html.contains("left: 32px; top: 32px"), "{html}");
    }

    #[test]
    fn shift_escape_leaves_the_sheet_and_plain_escape_never_does() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, block_keys) = sheet_block_targets(&opened);
        assert!(dioxus_ssr::render(&dom).contains("block-active"));

        // the sheet opens thinking; i writes, and rung one climbs back:
        // insert → normal, the caret turning box; the sheet holds
        // (adr/2026-08-escape-ladder-editor-wide-mode.md)
        press(
            &mut dom,
            block_keys,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="caret-box""#),
            "writing: the bar"
        );
        press(&mut dom, block_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="caret-box""#), "{html}");
        assert!(html.contains(r#"class="sheet""#), "{html}");

        // and there the plain key stops: pressing it again is the reflex
        // of checking the mode, and it costs nothing — the block still
        // holds the caret, the sheet still stands
        // (adr/2026-08-shift-escape-leaves-the-note.md)
        press(&mut dom, block_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "{html}");
        assert!(html.contains(r#"class="sheet""#), "{html}");

        // shift+Escape is the way out, and it takes the sheet with it:
        // the block renders, the sheet closes, the card goes back
        press(&mut dom, block_keys, Key::Escape, Modifiers::SHIFT);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("block-active"), "{html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");

        // pressed again over the bare table it finds nothing to close
        press(&mut dom, keys, Key::Escape, Modifiers::SHIFT);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="table""#), "{html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");

        // and the logs' note wakes with its own cursor, still thinking —
        // the mode survives the whole trip
        press(
            &mut dom,
            keys,
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "{html}");
        assert!(html.contains(r#"class="caret-box""#), "{html}");
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

        // dragging the raised card out into the margin left of the index
        // card drags the tether's card end with it: alpha lands at
        // (−168, 52), its right edge 8px from the pane's left edge
        mouse(&mut dom, "mousedown", raised, (0.0, 0.0));
        mouse(&mut dom, "mousemove", pane, (-200.0, 20.0));
        mouse(&mut dom, "mouseup", pane, (-200.0, 20.0));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("left: 8px; top: 80px; width: 182px"),
            "the tether followed the drag: {html}"
        );

        // panning under the sheet moves card and tether together
        mouse(&mut dom, "mousedown", pane, (200.0, 200.0));
        mouse(&mut dom, "mousemove", pane, (190.0, 180.0));
        mouse(&mut dom, "mouseup", pane, (190.0, 180.0));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("left: -2px; top: 60px; width: 192px"),
            "the tether followed the pan: {html}"
        );
        assert!(
            html.contains(
                r#"style="left: 190px; top: 130px; width: 900px; height: 540px""#
            ),
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
    fn closing_a_sheet_with_no_active_block_hands_focus_to_the_pane() {
        // regression: the effect never read `sheet`, so `editing` alone
        // picked the target. Closing this sheet reactivates the logs'
        // daily note, which wakes with its own last block active
        // (adr/2026-08-cursor-always-in-the-note.md) — but the table
        // screen with no sheet open never mounts a sink for it
        // (`blocks_view` only mounts on the logs screen or behind an open
        // sheet), so the old code aimed at a sink nothing had ever mounted
        // and focus was stranded on `<body>`, the table pane's own chords
        // (Ctrl+2, Ctrl+D, Ctrl+P) going dead
        // (adr/2026-09-sheet-and-screen-join-the-focus-effect.md).
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let mutations = click_for_mutations(&mut dom, clicks[CHROME_TABLE]);
        let downs = listeners(&mutations, "mousedown");
        let (pane_target, cards) = (downs[0], downs[1..].to_vec());
        let keys = listeners(&mutations, "keydown")[0];
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[0],
        );

        // the sabotaged lookup opens the sheet over a closed editor: no
        // active block to reactivate the sink with
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute("DELETE FROM notes WHERE id = 'alpha'", [])
            .expect("the sabotage succeeds");
        open_sheet_on(&mut dom, pane_target, cards[0]);
        block_on(settle(&mut dom));
        assert!(
            !dioxus_ssr::render(&dom).contains("block-active"),
            "the failed lookup leaves nothing active: {}",
            dioxus_ssr::render(&dom)
        );

        let before = focused.load(Ordering::SeqCst);
        // Shift+Escape at the pane's own keydown closes the sheet directly,
        // the same chord `plain_escape_leaves_the_sheet_open_and_shift_escape_closes_it`
        // exercises
        press(&mut dom, keys, Key::Escape, Modifiers::SHIFT);
        block_on(settle(&mut dom));
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="sheet""#),
            "the sheet closed"
        );
        assert!(
            focused.load(Ordering::SeqCst) > before,
            "the pane asked for focus once the sheet closed, even though \
             the reopened daily note came back active"
        );
    }

    #[test]
    fn a_sheet_with_an_active_block_keeps_the_sink_focused() {
        // the fix's other half: a sheet that does hold an active block
        // must keep routing to the sink — the table screen mounts one for
        // exactly as long as the sheet stands
        // (adr/2026-09-sheet-and-screen-join-the-focus-effect.md)
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let clicks = listeners(&mutations, "click");
        // registration order established by
        // `the_sink_takes_focus_back_when_an_overlay_closes`: the pane,
        // then the sink
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[1],
        );
        block_on(settle(&mut dom));

        let (pane, cards) = table_targets(&mut dom, &clicks);
        let before = focused.load(Ordering::SeqCst);
        open_sheet_on(&mut dom, pane, cards[0]);
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom).contains("block-active"),
            "the sheet opens with its last block awake: {}",
            dioxus_ssr::render(&dom)
        );
        assert!(
            focused.load(Ordering::SeqCst) > before,
            "a sheet with an active block still routes focus to the sink"
        );
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

    /// A good lookup resolves the notice its predecessor left standing —
    /// no gesture, the condition ending is the resolution
    /// (`adr/2026-09-index-notices-resolve-on-a-good-lookup.md`).
    #[test]
    fn a_later_sheet_on_a_real_card_clears_a_standing_index_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute("DELETE FROM notes WHERE id = 'alpha'", [])
            .expect("the sabotage succeeds");

        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(
            dioxus_ssr::render(&dom)
                .contains("sheet: no note has the id alpha"),
            "the notice is standing before the good lookup"
        );

        // capture-idea's row is untouched: this lookup succeeds and
        // resolves the sabotaged one's leftover notice on the way
        open_sheet_on(&mut dom, pane, cards[1]);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("sheet: no note has the id alpha"), "{html}");
    }

    /// Escape at the bottom of the sheet's ladder acknowledges an index
    /// notice the same as any other — the manual out the ADR keeps
    /// alongside resolution.
    #[test]
    fn escape_over_the_sheet_acknowledges_a_standing_index_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);

        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute("DELETE FROM notes WHERE id = 'alpha'", [])
            .expect("the sabotage succeeds");

        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(
            dioxus_ssr::render(&dom)
                .contains("sheet: no note has the id alpha"),
            "the notice is standing before Escape"
        );

        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("sheet: no note has the id alpha"), "{html}");
        assert!(
            html.contains(r#"class="sheet""#),
            "plain Escape leaves the sheet open: {html}"
        );
    }

    /// A failed lookup's bare sheet must not swallow the retry: the same
    /// card clicked again re-runs the lookup, and once the index heals
    /// that read is exactly the one that resolves the standing notice
    /// (adr/2026-09-index-notices-resolve-on-a-good-lookup.md). Pre-fix,
    /// `open_sheet`'s same-id early return fired before the lookup and
    /// left the error sheet and its notice both standing forever.
    #[test]
    fn reclicking_the_same_card_after_the_index_heals_opens_and_resolves() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);

        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute("UPDATE notes SET id = 'broken' WHERE id = 'alpha'", [])
            .expect("the sabotage succeeds");

        // the failed open remounts the card as the raised one, so the
        // retry click targets the raised card the mutations hand back —
        // the element the user actually sees and clicks again
        let mutations = open_sheet_on(&mut dom, pane, cards[0]);
        let raised = listeners(&mutations, "mousedown")[0];
        assert!(
            dioxus_ssr::render(&dom)
                .contains("sheet: no note has the id alpha"),
            "the notice is standing before the retry"
        );

        saboteur
            .execute("UPDATE notes SET id = 'alpha' WHERE id = 'broken'", [])
            .expect("the index heals");

        open_sheet_on(&mut dom, pane, raised);
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("sheet: no note has the id alpha"),
            "the retried lookup resolved the notice: {html}"
        );
        assert!(html.contains(r#"class="sheet""#), "{html}");
    }

    /// The other three index-read reporters resolve the same way: a lookup
    /// that fails while a sheet is open reports the notice inside it, and
    /// the same lookup succeeding — no gesture beyond running the command
    /// again — resolves it (`adr/2026-09-index-notices-resolve-on-a-good-lookup.md`).
    /// A sheet already on real content, not an error sheet, is the vehicle:
    /// it keeps a place to render the notice line without the sheet's own
    /// lookup being what fails.
    #[test]
    fn filter_cards_resolves_a_standing_index_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        replace_database_with_a_directory(vault.path());
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "filter cards");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("notice-warning"),
            "the failed lookup reports: {html}"
        );

        std::fs::remove_dir(vault.path().join(".index/index.db"))
            .expect("the sabotage lifts");
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "filter cards");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("notice-warning"),
            "the good lookup resolves it: {html}"
        );
        assert!(
            html.contains(r#"class="sheet""#),
            "the sheet stands: {html}"
        );
    }

    /// The switcher's own index read is a notice source like every
    /// other, resolving on the next good read
    /// (adr/2026-09-index-notices-resolve-on-a-good-lookup.md). The
    /// broken read costs the typed half alone: the overlay still opens.
    #[test]
    fn the_switcher_resolves_a_standing_index_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        replace_database_with_a_directory(vault.path());
        let (_input, picker_keys, _) = open_switcher(&mut dom, keys);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("notice-warning"),
            "the failed lookup reports: {html}"
        );
        assert!(html.contains(">open note<"), "it opened anyway: {html}");
        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());

        std::fs::remove_dir(vault.path().join(".index/index.db"))
            .expect("the sabotage lifts");
        open_switcher(&mut dom, keys);
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("notice-warning"),
            "the good lookup resolves it: {html}"
        );
        assert!(
            html.contains(r#"class="sheet""#),
            "the sheet stands: {html}"
        );
    }

    #[test]
    fn insert_link_resolves_a_standing_index_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, block_keys) = sheet_block_targets(&opened);

        replace_database_with_a_directory(vault.path());
        press(&mut dom, block_keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("notice-warning"),
            "the failed lookup reports: {html}"
        );

        std::fs::remove_dir(vault.path().join(".index/index.db"))
            .expect("the sabotage lifts");
        let mutations = press_for_mutations(
            &mut dom,
            block_keys,
            ctrl_l(),
            Modifiers::CONTROL,
        );
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("notice-warning"),
            "the good lookup resolves it: {html}"
        );
        assert!(html.contains("link-picker"), "the picker opened: {html}");
    }

    /// `open_templates` reads the filesystem, not the index, but shares the
    /// same resolve-on-a-good-lookup wiring as the other three
    /// (`adr/2026-09-index-notices-resolve-on-a-good-lookup.md`). It needs
    /// the logs screen, the one place its command is offered
    /// (`palette::available`), so the notice line it shares with the sheet
    /// renders in the centre pane instead.
    #[test]
    fn edit_template_resolves_a_standing_index_notice() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let templates = vault.path().join("templates");
        let hidden = vault.path().join("templates-hidden");
        std::fs::rename(&templates, &hidden).expect("the sabotage takes");

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "edit template");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("notice-warning"),
            "the failed read reports: {html}"
        );

        std::fs::rename(&hidden, &templates).expect("the sabotage lifts");
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "edit template");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("notice-warning"),
            "the good read resolves it: {html}"
        );
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

        lock_dir(&vault.path().join("permanent"), true);
        press(&mut dom, keys, Key::Escape, Modifiers::SHIFT);
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("permanent"), false);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="sheet""#),
            "an unsaved buffer holds the sheet open: {html}"
        );
        assert!(html.contains("notice-critical"), "{html}");
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

        lock_dir(&vault.path().join("time"), true);
        // the flush guard refuses: the day's buffer cannot reach disk, so
        // the logs stay up with the error rather than dropping the buffer
        click(&mut dom, clicks[FOOTER_BACKLINK]);
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("time"), false);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains("notice-critical"), "{html}");
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

        let (_, sink) = sheet_heading_targets(&mut dom, &opened);
        retype(&mut dom, sink, "= alpha renommé");
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
        let (block, keys) = sheet_heading_targets(&mut dom, &opened);

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
            source_of(&dom).contains(r#"= alpha[[digest]]"#),
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
        // the link is its own line-block now
        // (adr/2026-08-per-line-block-segmentation.md)
        let (block, keys) = activate_link(&mut dom, &clicks);

        place_caret(&mut dom, block, &hit, 3);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="table""#),
            "the chord crossed screens: {html}"
        );
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(html.contains("raised"), "{html}");
    }

    /// Today's day note with a `#link` to a file under the vault as its
    /// last line — the resource a follow hands to the launcher.
    fn day_linking_a_resource(vault: &Path) {
        std::fs::write(
            vault.join("time/2026-07-23.typ"),
            format!(
                "{}#link(\"/assets/slides.pdf\")[the slides]\n",
                time_note("2026-07-23", "daily")
            ),
        )
        .expect("the day is rewritten");
    }

    /// The resource line's targets. It sits where the fixture's `[[…]]` line
    /// sits, one click listener earlier: a `#link` is not a note link, so
    /// the footer under the note lists no outgoing row for it.
    fn activate_resource_link(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, ElementId) {
        activate_block(dom, clicks[BLOCK_LINK - 1])
    }

    #[test]
    fn ctrl_enter_on_a_resource_link_hands_the_vault_path_to_the_launcher() {
        let vault = temp_vault();
        day_linking_a_resource(vault.path());
        let (mut dom, clicks, hit, launched) =
            launcher_app(Some(vault.path().to_path_buf()), vec![]);
        let (block, keys) = activate_resource_link(&mut dom, &clicks);

        place_caret(&mut dom, block, &hit, 3);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        assert_eq!(
            *launched.lock().expect("the launch log never poisons"),
            vec![vault.path().join("assets/slides.pdf").display().to_string()],
            "the leading slash is the vault root"
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "nothing moved: {html}");
        assert!(!html.contains("open:"), "nothing to say: {html}");
    }

    #[test]
    fn a_launcher_that_will_not_start_is_a_notice_the_next_open_resolves() {
        let vault = temp_vault();
        day_linking_a_resource(vault.path());
        let (mut dom, clicks, hit, launched) = launcher_app(
            Some(vault.path().to_path_buf()),
            vec![Err("xdg-open: not found".to_string()), Ok(())],
        );
        let (block, keys) = activate_resource_link(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 3);

        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("open: xdg-open: not found"),
            "the refusal is on the line: {html}"
        );

        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("open:"), "the good open resolved it: {html}");
        assert_eq!(
            launched.lock().expect("the launch log never poisons").len(),
            2
        );
    }

    #[test]
    fn without_a_launcher_a_resource_link_is_inert() {
        // the headless app of the other tests injects none: the follow
        // does nothing rather than reaching for a process
        let vault = temp_vault();
        day_linking_a_resource(vault.path());
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_resource_link(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 3);
        press(&mut dom, keys, Key::Enter, Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(!html.contains("open:"), "{html}");
    }

    /// Beta's heading is `= beta\n[[alpha]][[2026-07-22]]` — one link
    /// of each reach, for the follows that start inside a sheet.
    fn beta_with_both_links(vault: &Path) {
        std::fs::write(
            vault.join("permanent/beta.typ"),
            format!("{}[[alpha]][[2026-07-22]]\n", note("beta")),
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
        let (block, keys) = sheet_link_targets(&mut dom, &opened);

        // inside `[[alpha]]`, the link line's own first bytes
        place_caret(&mut dom, block, &hit, 4);
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
        let (block, keys) = sheet_link_targets(&mut dom, &opened);

        // inside `[[2026-07-22]]`
        place_caret(&mut dom, block, &hit, 22);
        let landed = press_for_mutations(
            &mut dom,
            keys,
            Key::Enter,
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the linked day is selected: {html}"
        );

        // regression (final review): `select` pushes the sheet it closes,
        // so the visit log names beta, not the logs selection it stood over
        let logs_keys = listeners(&landed, "keydown")[0];
        let (_input, _picker_keys, _) = open_switcher(&mut dom, logs_keys);
        assert_eq!(
            picker_ids(&dom),
            ["beta", "2026-07-23"],
            "the followed-from sheet is the newest visit"
        );
    }

    #[test]
    fn ctrl_enter_on_a_dangling_link_goes_nowhere() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/gamma.typ"),
            format!("{}[[fantome]]\n", note("gamma")),
        )
        .expect("gamma is written");
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        // gamma sits last in id order
        let opened = open_sheet_on(&mut dom, pane, cards[3]);
        // the link is its own line-block now
        // (adr/2026-08-per-line-block-segmentation.md)
        let (block, keys) = sheet_link_targets(&mut dom, &opened);

        // inside `[[fantome]]`
        place_caret(&mut dom, block, &hit, IN_LINK);
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
            html.contains(r#"class="ember" title="open loops">3</span>"#),
            "the count stays"
        );
    }

    #[test]
    fn the_ember_is_inert_while_the_settings_overlay_stands() {
        // overlays never stack (adr/2026-08-settings-overlay.md): the
        // chrome sits above the settings box, so the ember stays clickable
        // there and must refuse rather than mount the loops list on top
        let vault = debt_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (overlay_keys, _) =
            open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        assert!(dioxus_ssr::render(&dom).contains(">settings<"));

        click(&mut dom, clicks[EMBER]);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("loops-list"), "the ember refused: {html}");
        assert!(html.contains(">settings<"), "settings still up: {html}");

        // the overlay closed, the ember answers again
        press(&mut dom, overlay_keys, Key::Escape, Modifiers::empty());
        click(&mut dom, clicks[EMBER]);
        assert!(dioxus_ssr::render(&dom).contains("loops-list"));
    }

    /// The overlay is rendered at the top level, not inside the logs
    /// screen's block alone, so the ember reaches it from the table too
    /// (adr/2026-08-palette-order-and-overlay-placement.md).
    #[test]
    fn the_ember_opens_the_flat_list_from_the_table_screen_too() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[CHROME_TABLE]);
        assert!(!dioxus_ssr::render(&dom).contains("loops-list"));

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

        click(&mut dom, clicks[EMBER]);
        assert!(!dioxus_ssr::render(&dom).contains("loops-list"));
    }

    /// The palette's "open loops" command reaches the same overlay from
    /// the table screen — not only the ember
    /// (adr/2026-08-palette-order-and-overlay-placement.md).
    #[test]
    fn the_palette_opens_the_loops_list_from_the_table_screen() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, table_keys) = table_targets_with_keys(&mut dom, &clicks);

        let (input, palette_keys) = open_palette(&mut dom, table_keys);
        type_into(&mut dom, input, "open loops");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("loops-list"), "{html}");
        assert!(html.contains("mystere · typeless"), "{html}");
    }

    #[test]
    fn the_loops_overlay_closes_on_a_click() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, own_click, _) = open_loops_overlay(&mut dom, clicks[EMBER]);
        assert!(dioxus_ssr::render(&dom).contains("loops-list"));

        click(&mut dom, own_click);
        assert!(!dioxus_ssr::render(&dom).contains("loops-list"));
    }

    #[test]
    fn the_loops_overlays_own_escape_closes_it() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);

        // only Escape is answered here; a plain key leaves the list up
        press(
            &mut dom,
            own_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert!(dioxus_ssr::render(&dom).contains("loops-list"));

        press(&mut dom, own_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains("loops-list"));
    }

    /// The table pane's own ladder carries the same `Key::Escape if
    /// loops_open()` fallback rung the logs pane's ladder has, so Escape
    /// closes the loops overlay from the table screen even when it is
    /// driven through the pane's own keydown rather than the overlay's
    /// (adr/2026-08-palette-order-and-overlay-placement.md).
    #[test]
    fn the_table_pane_escape_also_closes_the_loops_overlay() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, table_keys) = table_targets_with_keys(&mut dom, &clicks);

        click(&mut dom, clicks[EMBER]);
        assert!(dioxus_ssr::render(&dom).contains("loops-list"));

        press(&mut dom, table_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains("loops-list"));
    }

    /// A loop line carries the path of the note that owes the debt, and a
    /// click or Enter on it opens that note directly — not just the
    /// palette's arrow/Enter grammar over the list itself
    /// (adr/2026-09-loop-lines-open-their-notes.md).
    #[test]
    fn arrows_move_the_loops_highlight_and_clamp_at_both_ends() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);

        let selected_line = |dom: &VirtualDom| {
            let html = dioxus_ssr::render(dom);
            html.split("loops-line-open selected\">")
                .nth(1)
                .and_then(|rest| rest.split('<').next())
                .map(str::to_string)
                .unwrap_or_else(|| panic!("no highlighted row: {html}"))
        };
        assert_eq!(
            selected_line(&dom),
            "mystere · typeless",
            "the first row starts lit"
        );

        // three rows: down past the end clamps on the last one
        for _ in 0..5 {
            press(&mut dom, own_keys, Key::ArrowDown, Modifiers::empty());
        }
        assert_eq!(
            selected_line(&dom),
            "capture-zettel · still open",
            "arrow down clamps at the last row"
        );

        // and up past the start clamps back on the first
        for _ in 0..5 {
            press(&mut dom, own_keys, Key::ArrowUp, Modifiers::empty());
        }
        assert_eq!(
            selected_line(&dom),
            "mystere · typeless",
            "arrow up clamps at the first row"
        );
    }

    #[test]
    fn enter_on_the_loops_overlay_opens_the_highlighted_notes_sheet() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);

        // the first row is lit by default: mystere, a typeless permanent
        // note — nowhere to open before v1's table, which is exactly the
        // objection this decision supersedes
        press(&mut dom, own_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("loops-list"), "the overlay closed: {html}");
        assert!(html.contains(r#"class="sheet""#), "{html}");
        assert!(
            html.contains("mystere"),
            "the typeless note's own sheet opened: {html}"
        );
    }

    #[test]
    fn a_click_on_a_loops_row_opens_its_note_not_only_the_overlay() {
        let vault = debt_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, rows) = open_loops_overlay(&mut dom, clicks[EMBER]);

        // rank 1: "linky → ghost · dangling" — the id on the line is the
        // source that owes the link, not the missing target
        click(&mut dom, rows[1]);
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("loops-list"),
            "a row click does more than close the overlay: {html}"
        );
        assert!(html.contains(r#"class="sheet""#), "a sheet opened: {html}");
        assert!(
            !html.contains("no note has the id"),
            "the source note exists, so no lookup notice: {html}"
        );
        assert!(
            html.contains("linky"),
            "the dangling link's source opened, not its target: {html}"
        );
    }

    /// The loops list reads the live signal, not a snapshot frozen at open:
    /// a debt resolved from outside while the overlay is still up can
    /// shrink the list out from under a highlighted rank the arrows had
    /// clamped against the *old* length. Enter over that stale rank must
    /// find nothing and do nothing, never panic or open the wrong note.
    #[test]
    fn enter_on_a_stale_highlight_after_the_list_shrinks_is_a_no_op() {
        let vault = debt_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);

        // the last of the three rows: capture-zettel, "still open"
        for _ in 0..2 {
            press(&mut dom, own_keys, Key::ArrowDown, Modifiers::empty());
        }

        // summarized from outside the app, the way the watcher would see
        // any other edit — the debt it owed is gone
        std::fs::write(
            vault.path().join("capture/capture-zettel.typ"),
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"capture-zettel\", created: \"2026-07-23\")\n\
             \n== Summary\n\nrésumé\n\n== Original\n\ncollé du navigateur\n",
        )
        .expect("the capture is summarized");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Touched {
                category: NoteCategory::Capture,
                path: PathBuf::from("capture/capture-zettel.typ"),
            }],
        );
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("still open"),
            "the summary resolved the debt: {html}"
        );

        // the highlight still names the row that used to be last
        press(&mut dom, own_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("loops-list"),
            "a stale highlight past the shrunk list opens nothing: {html}"
        );
        assert!(!html.contains(r#"class="sheet""#), "{html}");
    }

    /// A loop line owed by a time note lands on the logs — rail, calendar
    /// and crumbs — never in a table card sheet floating with no card
    /// behind it (adr/2026-09-loop-lines-open-their-notes.md).
    #[test]
    fn a_time_notes_loop_line_lands_on_the_logs_not_a_sheet() {
        let vault = temp_vault();
        // a rail-known daily owing a dangling link: the loops list names
        // it by its source path
        std::fs::write(
            vault.path().join("time/2026-07-20.typ"),
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"2026-07-20\", type: \"daily\", \
             created: \"2026-07-20\")\n\
             \n= 2026-07-20\n\n[[fantome]]\n",
        )
        .expect("the indebted day note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        // from the table screen, so the switch to the logs is observable
        click(&mut dom, clicks[CHROME_TABLE]);
        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);
        press(&mut dom, own_keys, Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "no sheet: {html}");
        // the note's own body (its dangling wiki link) only renders once
        // the note is open in the logs editor — the rail row alone never
        // shows it, so this is the landing, not the listing
        assert!(
            html.contains("fantome"),
            "the day note opened on the logs: {html}"
        );
    }

    /// A typeless time note is exactly the loops case the rail excludes:
    /// `select`'s `exists` says no, the file says yes, and the file wins
    /// (`open_selected`'s disk fallback) — the note opens in the logs
    /// editor instead of a bare "press enter to start one".
    #[test]
    fn a_typeless_time_notes_loop_line_opens_its_file_in_the_logs() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-19.typ"),
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"2026-07-19\", created: \"2026-07-19\")\n\
             \n= presque un jour\n",
        )
        .expect("the typeless day note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);
        press(&mut dom, own_keys, Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "no sheet: {html}");
        assert!(
            html.contains("presque un jour"),
            "the file itself opened: {html}"
        );
    }

    /// A time file whose stem no scale can parse has no place the logs
    /// could put it — the sheet, which can show any file, is the fallback.
    #[test]
    fn a_time_file_with_an_unparseable_stem_falls_back_to_the_sheet() {
        let vault = temp_vault();
        std::fs::write(vault.path().join("time/notes.typ"), "= sans place\n")
            .expect("the misfiled note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);
        press(&mut dom, own_keys, Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="sheet""#), "a sheet opened: {html}");
        assert!(html.contains("sans place"), "with the file's text: {html}");
    }

    /// A refused flush keeps everything where it was: `select` returns
    /// before landing, and the loop's Enter must not switch the screen
    /// onto a selection that never changed (`open_id`'s own guard).
    #[test]
    fn a_refused_flush_keeps_a_time_loop_line_from_landing() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-20.typ"),
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"2026-07-20\", type: \"daily\", \
             created: \"2026-07-20\")\n\
             \n= 2026-07-20\n\n[[fantome]]\n",
        )
        .expect("the indebted day note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        // an unsaved edit the locked directory will refuse to flush
        let (input, _) = activate_heading(&mut dom, &clicks);
        type_into(&mut dom, input, "= pas encore sauvé\n");
        lock_dir(&vault.path().join("time"), true);

        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);
        press(&mut dom, own_keys, Key::Enter, Modifiers::empty());
        lock_dir(&vault.path().join("time"), false);

        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("fantome"),
            "the refused flush kept the selection, so no landing: {html}"
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
            57,
            "2 chrome icons + 3 header + 3 seasons + 5 gutters + 31 days \
             + 2 footer links + 4 blocks (the preamble, blank line, \
             heading and link above the note's own trailing empty line — \
             the one active by default, so it alone carries no click \
             listener — each now its own pane with its own click; \
             adr/2026-08-css-draws-the-markup.md) + 2 crumbs \
             + 5 rail — the active widget listens for presses, not \
             clicks: {html}"
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

    /// The broken `#let x = (` is its own block now, compiled and cached
    /// on its own — a bad block's failure stays on that one block instead
    /// of taking its siblings down with it, the way the old merged region
    /// used to (adr/2026-08-css-draws-the-markup.md).
    #[test]
    fn a_broken_block_fails_only_itself() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let switched = click_for_mutations(&mut dom, clicks[RAIL_DAY_21]);
        // the note's own trailing empty line is what wakes active
        // (adr/2026-08-cursor-always-in-the-note.md)
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(
            html.contains(RENDERED_NOTE),
            "the preamble block compiles fine on its own: {html}"
        );

        // the broken block is the only one whose shape changed from day
        // 23's fixture (a styled `Pane::Css` link line there, a compiled
        // `Pane::Fragment` here), so it is the only block this switch
        // mounts a fresh click listener for
        // (adr/2026-08-css-draws-the-markup.md)
        let broken = listeners(&switched, "click")[0];
        click(&mut dom, broken);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("render-error"), "{html}");
        assert!(html.contains("#let x = ("), "the broken source: {html}");
    }

    #[test]
    fn a_vanished_note_shows_the_read_error() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join("time/2026-07-22.typ"))
            .expect("the note exists before the click");
        click(&mut dom, clicks[RAIL_DAY_22]);
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-warning"), "{html}");
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
    fn a_missing_template_reports_the_create_error_and_escape_dismisses_it() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join("templates/daily.typ"))
            .expect("remove the template");
        click(&mut dom, clicks[day_cell(24)]);
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-warning"), "{html}");
        assert!(html.contains("UnknownTemplate"), "{html}");
        assert!(!vault.path().join("time/2026-07-24.typ").exists());

        // a warning persists until dismissed — navigation is not a gesture
        // (adr/2026-08-status-surface-owns-notices.md)
        click(&mut dom, clicks[RAIL_DAY_23]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("UnknownTemplate"), "navigation keeps it");

        // shift+Escape belongs to the note it leaves: it must not fall
        // down the ladder and acknowledge on its way out
        // (adr/2026-08-shift-escape-leaves-the-note.md)
        click(&mut dom, clicks[day_cell(24)]);
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::SHIFT);
        assert!(
            dioxus_ssr::render(&dom).contains("UnknownTemplate"),
            "shift held: the notice stands"
        );

        // the empty day has no block to close, so Escape reaches the
        // ladder's bottom and acknowledges
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("UnknownTemplate"), "escape dismisses it");
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
        // the heading block "= 2026-07-23\n[[2026-07-22]]\n" ends in a
        // newline, so the caret's line is the empty last one: a source
        // line holding nothing but the drawn caret
        // (adr/2026-08-cursor-always-in-the-note.md)
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(
                r#"<div class="source-line"><span class="caret-box""#
            ),
            "a box caret: a new file starts thinking: {html}"
        );
        // the renderer announces the caret's mount; the scroll-into-view
        // asks and the headless refusal is absorbed
        mount(&mut dom, listeners(&mutations, "mounted")[2]);
        block_on(settle(&mut dom));
    }

    #[test]
    fn clicking_a_block_opens_its_source_in_place() {
        // the note opens with its own trailing empty line already the
        // source (adr/2026-08-cursor-always-in-the-note.md); clicking any
        // other block moves the source to exactly that block — the block
        // actually clicked, not a neighbour
        // (adr/2026-08-css-draws-the-markup.md) — and the previously
        // active line renders as CSS markup in its place, being a blank
        // line
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "born editing: {html}");

        let mutations = click_for_mutations(&mut dom, clicks[BLOCK_LINK]);
        // the renderer announces the mount and the textarea asks for focus;
        // the fake backing refuses, which is all the handler has to absorb
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("2026-07-22"), "the link line source: {html}");
        assert!(
            html.contains("mk-blank"),
            "the previously active line renders as CSS in its place: {html}"
        );
    }

    /// The number of block panes (Css or SVG) `html` draws before and
    /// after its one `block-active` widget, in document order — the
    /// per-block replacement for the old cursor-split's above/below region
    /// counts (adr/2026-08-css-draws-the-markup.md).
    fn panes_around_active(html: &str) -> (usize, usize) {
        let marker = r#"class="block-active"#;
        let at = html.find(marker).expect("exactly one active block");
        let pane = r#"class="block block-"#;
        (
            html[..at].matches(pane).count(),
            html[at..].matches(pane).count(),
        )
    }

    /// With the caret mid-note, every other block still renders — some
    /// above the active one, some below it — each its own click-activated
    /// pane instead of one merged region on each side
    /// (adr/2026-08-css-draws-the-markup.md).
    #[test]
    fn blocks_render_on_both_sides_of_the_caret_mid_note() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        activate_heading(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert_eq!(
            html.matches(r#"class="block-active"#).count(),
            1,
            "one active source pane: {html}"
        );
        // preamble and the blank line above the heading; the link line and
        // the note's own trailing blank line below it
        assert_eq!(panes_around_active(&html), (2, 2), "{html}");
    }

    /// With the caret on the note's first block, there is nothing above it
    /// to render — every other block renders below instead
    /// (adr/2026-08-css-draws-the-markup.md).
    #[test]
    fn no_block_renders_above_the_caret_on_the_first_line() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        activate_preamble(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert_eq!(
            html.matches(r#"class="block-active"#).count(),
            1,
            "{html}"
        );
        assert_eq!(
            panes_around_active(&html),
            (0, 4),
            "nothing above the first block; the rest render below: {html}"
        );
    }

    /// With the caret on the note's last block — its default position on
    /// open (adr/2026-08-cursor-always-in-the-note.md) — there is nothing
    /// below it to render.
    #[test]
    fn no_block_renders_below_the_caret_on_the_last_line() {
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert_eq!(
            html.matches(r#"class="block-active"#).count(),
            1,
            "{html}"
        );
        assert_eq!(
            panes_around_active(&html),
            (4, 0),
            "everything renders above the last block; nothing below: {html}"
        );
    }

    // -- selected_pane and piece_span: a covered non-active block's own
    //    rendering, in isolation from the whole app
    //    (adr/2026-08-visual-selection-drawn-across-lines.md) -------------

    #[test]
    fn piece_span_reduces_selected_and_text_pieces() {
        assert_eq!(
            piece_span(caret::Piece::Selected {
                start: 3,
                text: "hi".to_string()
            }),
            (true, 3, "hi".to_string())
        );
        assert_eq!(
            piece_span(caret::Piece::Text {
                start: 5,
                text: "lo".to_string()
            }),
            (false, 5, "lo".to_string())
        );
    }

    /// `layout_selected` never emits these three, but the match stays
    /// exhaustive rather than trusting that blind — proven dead here
    /// instead of trusted never to arrive.
    #[test]
    fn piece_span_never_highlights_a_caret_box_or_preview() {
        assert_eq!(piece_span(caret::Piece::Caret), (false, 0, String::new()));
        assert_eq!(
            piece_span(caret::Piece::CaretBox {
                start: 2,
                cluster: "x".to_string()
            }),
            (false, 2, "x".to_string())
        );
        assert_eq!(
            piece_span(caret::Piece::Preview {
                start: 4,
                text: "y".to_string()
            }),
            (false, 4, "y".to_string())
        );
    }

    /// `markup::tint` never emits these three for the `Piece::Text` input
    /// `markup_lines` feeds it, but the match stays exhaustive rather than
    /// trusting that blind — proven dead here instead of trusted never to
    /// arrive, the same guard `piece_span` carries for `Pane::Selected`.
    #[test]
    fn css_piece_span_reduces_every_piece_kind() {
        assert_eq!(
            css_piece_span(caret::Piece::Text {
                start: 3,
                text: "hi".to_string()
            }),
            (3, "hi".to_string())
        );
        assert_eq!(
            css_piece_span(caret::Piece::Selected {
                start: 5,
                text: "lo".to_string()
            }),
            (5, "lo".to_string())
        );
        assert_eq!(
            css_piece_span(caret::Piece::Preview {
                start: 7,
                text: "up".to_string()
            }),
            (7, "up".to_string())
        );
        assert_eq!(
            css_piece_span(caret::Piece::CaretBox {
                start: 2,
                cluster: "x".to_string()
            }),
            (2, "x".to_string())
        );
        assert_eq!(css_piece_span(caret::Piece::Caret), (0, String::new()));
    }

    /// One block fully inside the selection: its whole content comes back
    /// as one `Selected` line, start included.
    #[test]
    fn selected_pane_covers_a_fully_selected_block() {
        let text = "= heading\nafter\n";
        let blocks = blocks::segment(text);
        // blocks[0] is "= heading", covered end to end
        let selection = 0..text.len();
        let Pane::Selected { start, lines, .. } =
            selected_pane(text, &blocks[0], &selection)
        else {
            panic!("selected_pane always answers Selected");
        };
        assert_eq!(start, 0);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(
            lines[0].pieces,
            vec![caret::Piece::Selected {
                start: 0,
                text: "= heading".to_string(),
            }]
        );
    }

    /// The boundary block, only partly inside the selection: the covered
    /// suffix is `Selected`, the rest stays plain — the "raw ends" case
    /// `adr/2026-08-visual-selection-is-the-anchor.md` named up front.
    #[test]
    fn selected_pane_covers_only_the_reached_slice_of_a_boundary_block() {
        let text = "= heading\nafter\n";
        let blocks = blocks::segment(text);
        // the selection starts three bytes into the heading and runs off
        // the end of the note
        let selection = 3..text.len();
        let Pane::Selected { lines, .. } =
            selected_pane(text, &blocks[0], &selection)
        else {
            panic!("selected_pane always answers Selected");
        };
        assert_eq!(
            lines[0].pieces,
            vec![
                caret::Piece::Text {
                    start: 0,
                    text: "= h".to_string(),
                },
                caret::Piece::Selected {
                    start: 3,
                    text: "eading".to_string(),
                },
            ]
        );
    }

    /// A blank block the selection never reaches: no bytes to highlight and
    /// none to leave plain either — the block still answers one line, empty
    /// of pieces exactly as the active widget's own blank lines are, so the
    /// `.source-line` min-height carries it the same vertical space it
    /// would occupy compiled or active.
    #[test]
    fn selected_pane_of_an_untouched_blank_block_is_one_empty_line() {
        let text = "= heading\n\nafter\n";
        let blocks = blocks::segment(text);
        // blocks[1] is the blank line between the two paragraphs; the
        // selection sits entirely inside blocks[2] ("after"), well past it
        let after_start = blocks[2].range.start;
        let selection = after_start..text.len();
        let Pane::Selected { start, lines, .. } =
            selected_pane(text, &blocks[1], &selection)
        else {
            panic!("selected_pane always answers Selected");
        };
        assert_eq!(start, blocks[1].content().start);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].pieces.is_empty(), "{lines:?}");
    }

    /// A note opened straight through `Editor::open`, its caret placed by
    /// hand rather than through the vim grammar: `block_panes` is a pure
    /// function of the editor's own state, so the two boundary shapes below
    /// — nothing left to compile above the covered run, nothing left below
    /// it — are cheaper proven here than through a whole keystroke script.
    fn editor_with_selection(
        dir: &std::path::Path,
        text: &str,
        anchor_at: usize,
        head_at: usize,
    ) -> Editor {
        let file = dir.join("note.typ");
        std::fs::write(&file, text).expect("the fixture note is writable");
        let mut editor = Editor::open(file);
        editor.activate(anchor_at);
        editor.extend_to(head_at);
        editor
    }

    /// The selection's low end reaches the note's very first block: nothing
    /// is left to compile above the run of `Selected` panes, so `above`'s
    /// own compiled pane is skipped rather than mounted empty.
    #[test]
    fn block_panes_skips_the_compiled_region_when_the_run_starts_at_the_top() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let text = "= a\n\nb\n\nc\n";
        let blocks = blocks::segment(text);
        // anchor in block 0, head in block 2 ("b"): the run covers blocks
        // 0 and 1 whole, nothing precedes them
        let editor = editor_with_selection(
            dir.path(),
            text,
            0,
            blocks[2].content().start,
        );
        let panes = block_panes(
            &editor,
            dir.path(),
            RenderTheme::Paper(DEFAULT_SIZE),
            &mut FragmentCache::default(),
            false,
            false,
        )
        .expect("an open note always answers panes");
        assert!(
            matches!(panes.first(), Some(Pane::Selected { .. })),
            "no compiled pane precedes the run"
        );
    }

    /// The selection's high end reaches the note's very last block: nothing
    /// is left to compile below the run either.
    #[test]
    fn block_panes_skips_the_compiled_region_when_the_run_ends_at_the_bottom()
    {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        // no trailing newline: the last block ("c") is real content, not
        // the empty line a trailing newline would open
        // (adr/2026-08-cursor-always-in-the-note.md)
        let text = "= a\n\nb\n\nc";
        let blocks = blocks::segment(text);
        // anchor in the last block, head in block 2 ("b"): the run covers
        // blocks 3 and 4 whole, nothing follows them
        let editor = editor_with_selection(
            dir.path(),
            text,
            text.len(),
            blocks[2].content().start,
        );
        let panes = block_panes(
            &editor,
            dir.path(),
            RenderTheme::Paper(DEFAULT_SIZE),
            &mut FragmentCache::default(),
            false,
            false,
        )
        .expect("an open note always answers panes");
        assert!(
            matches!(panes.last(), Some(Pane::Selected { .. })),
            "no compiled pane follows the run"
        );
    }

    /// `V` across lines: the boundary block's own highlight widens to the
    /// whole line, matching what `d`/`y`/`c` already take
    /// (adr/2026-08-visual-selection-drawn-across-lines.md) — without
    /// `linewise`, the same setup leaves the boundary block's highlight
    /// ragged, only the bytes the raw anchor..head span actually reaches.
    #[test]
    fn block_panes_widens_the_boundary_block_under_linewise() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let text = "= heading\n\nafter\n";
        let blocks = blocks::segment(text);
        let file = dir.path().join("note.typ");
        std::fs::write(&file, text).expect("the fixture note is writable");
        let mut editor = Editor::open(file);
        // anchor three bytes into the heading — `place_at`, not `activate`,
        // so the anchor lands on the byte itself rather than snapping to
        // the woken block's end (`Editor::activate`'s own click semantics) —
        // head at the active block ("after"): the boundary block
        // ("= heading") is only reached from byte 3 onward by the raw span
        editor.place_at(3);
        editor.extend_to(blocks[2].content().start);

        let ragged = block_panes(
            &editor,
            dir.path(),
            RenderTheme::Paper(DEFAULT_SIZE),
            &mut FragmentCache::default(),
            false,
            false,
        )
        .expect("an open note always answers panes");
        let Some(Pane::Selected { start, lines, .. }) = ragged.first() else {
            panic!("the boundary block always answers Selected");
        };
        assert_eq!(*start, 0);
        assert_eq!(
            lines[0].pieces,
            vec![
                caret::Piece::Text {
                    start: 0,
                    text: "= h".to_string(),
                },
                caret::Piece::Selected {
                    start: 3,
                    text: "eading".to_string(),
                },
            ],
            "{lines:?}"
        );

        let widened = block_panes(
            &editor,
            dir.path(),
            RenderTheme::Paper(DEFAULT_SIZE),
            &mut FragmentCache::default(),
            false,
            true,
        )
        .expect("an open note always answers panes");
        let Some(Pane::Selected { start, lines, .. }) = widened.first() else {
            panic!("the boundary block always answers Selected");
        };
        assert_eq!(*start, 0);
        assert_eq!(
            lines[0].pieces,
            vec![caret::Piece::Selected {
                start: 0,
                text: "= heading".to_string(),
            }],
            "linewise widens the boundary block to the whole line: {lines:?}"
        );
    }

    /// A note whose every block — a heading, two plain lines, the blank
    /// lines between them — draws from the markup model: moving the caret
    /// changes only which block is `Pane::Source`, never which pane needs
    /// the compiled-Typst fallback, so the fragment cache the compiled
    /// panes would have touched stays exactly as `block_panes` found it
    /// (adr/2026-08-css-draws-the-markup.md).
    #[test]
    fn moving_the_caret_across_css_only_blocks_compiles_nothing() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let text = "= a\n\nb\n\nc\n";
        let file = dir.path().join("note.typ");
        std::fs::write(&file, text).expect("the fixture note is writable");
        let mut editor = Editor::open(file);
        let mut cache = FragmentCache::default();

        block_panes(
            &editor,
            dir.path(),
            RenderTheme::Paper(DEFAULT_SIZE),
            &mut cache,
            false,
            false,
        )
        .expect("an open note always answers panes");

        let blocks = blocks::segment(text);
        editor.activate(blocks[2].content().start);
        block_panes(
            &editor,
            dir.path(),
            RenderTheme::Paper(DEFAULT_SIZE),
            &mut cache,
            false,
            false,
        )
        .expect("an open note always answers panes");

        assert_eq!(
            format!("{cache:?}"),
            format!("{:?}", FragmentCache::default()),
            "every block drew from the markup model; nothing ever touched \
             the compiled-fallback cache"
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

        lock_dir(&vault.path().join("time"), true);
        // activating any other block flushes the born-active trailing
        // line first, and the failure is the notice
        click(&mut dom, clicks[BLOCK_LINK]);
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("time"), false);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-critical"), "{html}");
        assert!(html.contains("2026-07-23.typ"), "{html}");
    }

    #[test]
    fn caret_keys_stay_in_the_source_while_ctrl_chords_escape_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);

        // arrows move the caret, not the month grid below; a horizontal
        // move stays inside the one-line heading block
        // (adr/2026-08-per-line-block-segmentation.md)
        press(&mut dom, keys, Key::ArrowLeft, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("july 2026"), "no paging: {html}");
        // the caret box now splits the heading's own text into two spans,
        // so the block staying active (not paged away) is what "no slide"
        // means here
        assert!(html.contains("block-active"), "no slide: {html}");
        // enter in the source must not reach the create handler either
        press(&mut dom, keys, Key::Enter, Modifiers::empty());
        assert!(html.contains("block-active"), "still editing: {html}");

        // the todo chord still escapes the source instead of typing a
        // literal "t" (adr/2026-08-ctrl-t-toggles-the-todo.md)
        press(
            &mut dom,
            keys,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        assert!(
            source_of(&dom).contains("- [ ] "),
            "the chord toggled the todo, not typed a t: {}",
            source_of(&dom)
        );
    }

    /// The blank line between the fixture day note's meta and its heading
    /// renders as its own CSS block now, styled through the markup model
    /// rather than merged into a compiled Typst region
    /// (adr/2026-08-css-draws-the-markup.md).
    #[test]
    fn a_blank_line_renders_as_its_own_css_pane() {
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("mk-blank"), "{html}");
        // the two classes adjacent in one attribute, not merely both
        // present somewhere in the document: `block-blank` supplies the
        // blank line box and only means anything on the element that also
        // carries `mk-blank`, so an assertion that cannot tell those apart
        // still passes with the emitting condition inverted
        assert!(
            html.contains(r#"class="block block-css mk-blank block-blank""#),
            "the blank block keeps the same shape class an active blank \
             line sits inside, on its own element: {html}"
        );
        // every `block-blank` sits in that pair and nowhere else, so an
        // inverted condition that moved the box onto `mk-h1`/`mk-line`
        // blocks fails here. Not `mk-blank`'s own count: the *active*
        // blank block wears `mk-blank` without `block-blank` on purpose,
        // taking its box from `.block-active` instead.
        assert_eq!(
            html.matches("block-blank").count(),
            html.matches("mk-blank block-blank").count(),
            "no non-blank block wears the blank line box: {html}"
        );
    }

    #[test]
    fn switching_blocks_flushes_and_moves_the_source() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        retype(&mut dom, sink, "= renamed\n");

        // switching to a different block flushes the edit before the new
        // source mounts — every block wakes on its own click now, the
        // preamble's directly (adr/2026-08-css-draws-the-markup.md)
        let landed = click_for_mutations(&mut dom, clicks[BLOCK_PREAMBLE]);
        let preamble_keys = listeners(&landed, "keydown")[0];
        assert_ne!(preamble_keys, sink, "the source moved to a fresh widget");
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

        // caret on the heading's line: up slides onto the blank line-block
        // above it — a fresh widget, its own keydown sink — one more up
        // from there slides into the preamble
        // (adr/2026-08-per-line-block-segmentation.md)
        place_caret(&mut dom, block, &hit, 0);
        let slid = press_for_mutations(
            &mut dom,
            keys,
            Key::ArrowUp,
            Modifiers::empty(),
        );
        let keys = listeners(&slid, "keydown")[0];
        press(&mut dom, keys, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("#import"), "the preamble source: {html}");
        assert!(!html.contains("= 2026-07-23"), "one active block: {html}");
    }

    #[test]
    fn a_mid_block_caret_slides_nowhere_and_a_missed_press_lands_at_the_end() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_preamble(&mut dom, &clicks);

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
    fn escape_thinks_and_i_writes() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);

        // a new file starts thinking: a box caret, and unbound keys inert
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="caret-box""#), "{html}");
        press(
            &mut dom,
            sink,
            Key::Character("q".into()),
            Modifiers::empty(),
        );
        press(&mut dom, sink, Key::Enter, Modifiers::empty());
        assert_eq!(source_of(&dom), before, "normal mode never types");

        // i writes, the caret turning bar; the renderer announces the
        // fresh bar and its scroll-into-view absorbs the headless refusal
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        mount(&mut dom, listeners(&woken, "mounted")[0]);
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom).contains(r#"class="caret""#),
            "the bar is out"
        );
        press(
            &mut dom,
            sink,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert!(source_of(&dom).contains('x'), "{}", source_of(&dom));

        // and escape thinks again
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        assert!(
            dioxus_ssr::render(&dom).contains(r#"class="caret-box""#),
            "the box is back"
        );
    }

    #[test]
    fn insert_mode_enter_on_a_quote_line_leaves_it_literal() {
        // the `>` quote form is read by the template's `show par:` rule,
        // not expanded by the editor: Enter is an ordinary newline here.
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        retype(&mut dom, sink, "> Une idée _importante_. _Simone Weil_\n");

        assert_eq!(
            source_of(&dom),
            "> Une idée _importante_. _Simone Weil_\n"
        );
    }

    #[test]
    fn o_opens_a_line_below_through_the_widget() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // o from the heading's last line opens below and writes
        press(
            &mut dom,
            sink,
            Key::Character("o".into()),
            Modifiers::empty(),
        );
        type_keys(&mut dom, sink, "ouvert");
        assert_eq!(
            source_of(&dom),
            "= 2026-07-23\nouvert",
            "the line opened below the caret's line"
        );
    }

    #[test]
    fn gg_and_g_carry_the_caret_across_blocks() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // normal mode, then gg: the preamble block wakes with the caret
        // on its first line, still boxed
        press(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(
            source_of(&dom).contains("#import"),
            "the preamble is the source: {}",
            source_of(&dom)
        );
        assert!(html.contains(r#"class="caret-box""#), "{html}");

        // the widget remounted with the woken block: G goes to its fresh
        // sink and comes back to the note's own last line — a genuinely
        // empty one now that every line is its own block
        // (adr/2026-08-per-line-block-segmentation.md)
        let sink = listeners(&woken, "keydown")[0];
        press(
            &mut dom,
            sink,
            Key::Character("G".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "the last line woke: {html}");
        assert!(
            source_of(&dom).is_empty(),
            "the note's own trailing empty line: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn dd_cuts_to_the_clipboard_and_p_pastes_what_it_reads() {
        let vault = temp_vault();
        let (mut dom, clicks, written) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Ok("collée\n".to_string()),
        );
        // the link is its own line-block now
        // (adr/2026-08-per-line-block-segmentation.md); normal mode on
        // it, then dd: the line leaves for the register
        // (adr/2026-08-one-register-the-clipboard.md)
        let (_, sink) = activate_link(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(
            !source_of(&dom).contains("[["),
            "the line went: {}",
            source_of(&dom)
        );
        assert_eq!(
            *written.lock().expect("the write cell"),
            vec!["[[2026-07-22]]\n".to_string()],
            "linewise, newline carried"
        );

        // p pastes whatever the read seam answers, linewise below
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(source_of(&dom).contains("collée"), "{}", source_of(&dom));
    }

    #[test]
    fn dd_on_the_table_sheets_last_line_lands_on_the_heading_above() {
        // one vim grammar, one Editor shared by both mounts (src/ui.rs
        // sheet/pane split): the sheet opens with the note's own trailing
        // empty line active, and dd there must take the preceding newline
        // and wake the heading above rather than leave a blank line
        // behind, exactly as the main editor does
        // (adr/2026-08-cursor-always-in-the-note.md)
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards) = table_targets(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, sink) = sheet_block_targets(&opened);

        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );

        assert_eq!(
            source_of(&dom),
            "= alpha",
            "the heading woke, no blank line left behind"
        );
    }

    #[test]
    fn r_overwrites_through_the_widget_and_the_caret_stays_a_box() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // the heading's own line opens with the caret at its end (its
        // block's own line-block now — adr/2026-08-per-line-block-segmentation.md);
        // 0 moves it to the start, within the block
        press(
            &mut dom,
            sink,
            Key::Character("0".into()),
            Modifiers::empty(),
        );
        let before = source_of(&dom);

        // R overwrites the cluster under the caret rather than inserting
        press(
            &mut dom,
            sink,
            Key::Character("R".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("X".into()),
            Modifiers::empty(),
        );

        let after = source_of(&dom);
        assert_ne!(after, before, "the overwrite changed the source");
        assert!(after.starts_with('X'), "{after}");
        assert_eq!(
            after.len(),
            before.len(),
            "one cluster overwritten, none inserted"
        );

        // R draws the box caret, same as normal mode
        // (adr/2026-08-replace-mode-session-and-backspace.md)
        assert!(
            dioxus_ssr::render(&dom).contains(r#"class="caret-box""#),
            "{}",
            dioxus_ssr::render(&dom)
        );
    }

    #[test]
    fn dg_crosses_blocks_and_paste_declines_without_a_readable_clip() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // gg to the preamble, then dG: the whole note goes in one splice
        // across every block (adr/2026-08-editor-splice-cross-block.md)
        press(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let sink = listeners(&woken, "keydown")[0];
        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("G".into()),
            Modifiers::empty(),
        );
        assert_eq!(source_of(&dom), "", "the note emptied whole");
        assert!(!dioxus_ssr::render(&dom).contains("render-error"));

        // p without any clipboard seam quietly declines
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), "", "nothing to paste, nothing pasted");

        // and with a seam whose read answers emptiness, the same
        let (mut dom, clicks, _) =
            clipboard_app(Some(vault.path().to_path_buf()), Ok(String::new()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before);
    }

    #[test]
    fn a_successful_read_resolves_the_clipboard_warning() {
        let vault = temp_vault();
        let (mut dom, clicks) = clipboard_script_app(
            Some(vault.path().to_path_buf()),
            VecDeque::from([
                Err("read denied".to_string()),
                Ok("recovered".to_string()),
            ]),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);

        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(dioxus_ssr::render(&dom).contains("clipboard: read denied"));

        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(source_of(&dom).contains("recovered"));
        assert!(!dioxus_ssr::render(&dom).contains("clipboard: read denied"));
    }

    #[test]
    fn a_clipboard_read_failure_leaves_the_note_and_says_why() {
        let vault = temp_vault();
        let (mut dom, clicks, _) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Err("read denied".to_string()),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("clipboard: read denied — no text was read"),
            "the failed read must not stay silent: {html}"
        );
    }

    #[test]
    fn v_e_d_reads_like_the_sentence_it_is() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // normal, to the line's start, then v e: the first word lights up
        press(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        // gg woke another block: the widget remounted with a fresh sink
        let sink = listeners(&woken, "keydown")[0];
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("e".into()),
            Modifiers::empty(),
        );
        // the preamble is the Typst verdict — no markup role to merge in,
        // so the merged `class` attribute keeps a trailing space where the
        // role would otherwise sit (adr/2026-08-css-draws-the-markup.md)
        assert!(
            dioxus_ssr::render(&dom).contains(r#"class="sel ""#),
            "the span shows before the verb: {}",
            dioxus_ssr::render(&dom)
        );

        // o hops to the other end and back, the span holding
        press(
            &mut dom,
            sink,
            Key::Character("o".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("o".into()),
            Modifiers::empty(),
        );
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sel ""#));

        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );
        assert!(
            !source_of(&dom).starts_with("#import"),
            "the word went: {}",
            source_of(&dom)
        );
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="sel ""#),
            "and the selection collapsed"
        );
    }

    #[test]
    fn u_undoes_one_intent_and_ctrl_r_returns_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // one insert session is one intent
        press(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        type_keys(&mut dom, sink, "songe ");
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        assert!(source_of(&dom).contains("songe"));

        let undone = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("u".into()),
            Modifiers::empty(),
        );
        // the click that first woke the heading never dirtied the text,
        // so its checkpoint was a no-op step over
        // (adr/2026-08-undo-at-vim-grain.md): undo falls back to the
        // file-open checkpoint, waking that checkpoint's own last block —
        // the note's own trailing empty line
        // (adr/2026-08-cursor-always-in-the-note.md) — rather than the
        // heading itself, a fresh widget with its own keydown sink.
        // Either way "songe" is gone.
        assert!(
            !dioxus_ssr::render(&dom).contains("songe"),
            "one press undid the session"
        );

        let sink = listeners(&undone, "keydown")[0];
        press(
            &mut dom,
            sink,
            Key::Character("r".into()),
            Modifiers::CONTROL,
        );
        assert!(
            dioxus_ssr::render(&dom).contains("songe"),
            "ctrl+r brought it back"
        );
    }

    #[test]
    fn the_dot_repeats_through_the_widget() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // normal on the heading's first line, x then . . — three cuts
        press(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let sink = listeners(&woken, "keydown")[0];
        press(
            &mut dom,
            sink,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character(".".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character(".".into()),
            Modifiers::empty(),
        );
        assert!(
            source_of(&dom).starts_with("port"),
            "three cuts off #import: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn slash_searches_and_lands_in_a_rendered_block() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // / opens the one-line prompt over the active heading
        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("/".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"placeholder="/""#), "{html}");
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);

        // the pattern lives in the rendered preamble: enter jumps there,
        // waking the block (adr/2026-08-search-lands-through-place.md)
        type_into(&mut dom, prompt_input, "templates");
        press(&mut dom, prompt_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"placeholder="/""#), "the prompt closed");
        assert!(
            source_of(&dom).contains("#import"),
            "the preamble woke as source: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn escape_closes_the_search_prompt_untouched() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);

        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("/".into()),
            Modifiers::empty(),
        );
        let prompt_keys = listeners(&opened, "keydown")[0];

        // neither Escape nor Enter: the prompt's own match falls to its
        // wildcard arm and still swallows the key
        press(&mut dom, prompt_keys, Key::ArrowLeft, Modifiers::empty());
        assert!(
            dioxus_ssr::render(&dom).contains(r#"placeholder="/""#),
            "an unrelated key leaves the prompt open"
        );

        // an unbound ctrl chord still bubbles past the prompt instead of
        // being swallowed; nothing claims it, so nothing moves
        press(
            &mut dom,
            prompt_keys,
            Key::Character("z".into()),
            Modifiers::CONTROL,
        );
        assert_eq!(
            source_of(&dom),
            before,
            "the bubbled chord matched nothing"
        );

        press(&mut dom, prompt_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"placeholder="/""#), "{html}");
        assert_eq!(source_of(&dom), before, "nothing moved");

        // n walks on afterwards from the grammar's stored pattern — with
        // none committed it stays quietly put
        press(
            &mut dom,
            sink,
            Key::Character("n".into()),
            Modifiers::empty(),
        );
        assert_eq!(source_of(&dom), before);

        // a committed pattern the note lacks jumps nowhere either
        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("/".into()),
            Modifiers::empty(),
        );
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        type_into(&mut dom, prompt_input, "zzz");
        press(&mut dom, prompt_keys, Key::Enter, Modifiers::empty());
        assert_eq!(source_of(&dom), before);
    }

    #[test]
    fn a_composition_in_normal_mode_is_discarded() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);

        compose(&mut dom, sink, "compositionstart", "");
        compose(&mut dom, sink, "compositionupdate", "^");
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="compose"#),
            "no preview outside insert"
        );
        compose(&mut dom, sink, "compositionend", "ê");
        assert_eq!(source_of(&dom), before, "the commit was discarded");
    }

    #[test]
    fn the_mode_survives_a_boundary_slide() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, sink) = activate_heading(&mut dom, &clicks);

        place_caret(&mut dom, block, &hit, 0);
        press(&mut dom, sink, Key::ArrowUp, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        // the heading is its own line-block now: up slides onto the blank
        // line above it (adr/2026-08-per-line-block-segmentation.md)
        assert!(
            html.contains("block-active"),
            "slid off the heading: {html}"
        );
        assert!(
            html.contains(r#"class="caret-box""#),
            "still thinking after the slide: {html}"
        );
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

        press(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        press(&mut dom, sink, Key::Dead, Modifiers::empty());
        compose(&mut dom, sink, "compositionstart", "");
        compose(&mut dom, sink, "compositionupdate", "^");
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="compose"#),
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
            !dioxus_ssr::render(&dom).contains(r#"class="compose"#),
            "the preview is gone"
        );
    }

    #[test]
    fn a_dead_key_behind_a_verb_keeps_the_verb() {
        // the whole real sequence for d^ on a French layout: the dead
        // key's own keydown, the composition, the commit keystroke —
        // which WebKitGTK can send UNflagged (the spike's stray keydown)
        // — then the empty end and the real one. Unguarded, that stray
        // keydown reads as an unbound key and resets the pending d
        // (adr/2026-08-hidden-ime-sink.md).
        fn cut_with(flagged: bool, stray_between_ends: bool) -> String {
            let vault = temp_vault();
            let (mut dom, clicks, hit) =
                hit_app(Some(vault.path().to_path_buf()));
            let (block, sink) = activate_heading(&mut dom, &clicks);
            place_caret(&mut dom, block, &hit, 4);
            press(
                &mut dom,
                sink,
                Key::Character("d".into()),
                Modifiers::empty(),
            );
            press(&mut dom, sink, Key::Dead, Modifiers::empty());
            compose(&mut dom, sink, "compositionstart", "");
            compose(&mut dom, sink, "compositionupdate", "^");
            let commit = Key::Character(" ".into());
            if !stray_between_ends {
                if flagged {
                    press_composing(&mut dom, sink, commit.clone());
                } else {
                    press(&mut dom, sink, commit.clone(), Modifiers::empty());
                }
            }
            compose(&mut dom, sink, "compositionend", "");
            if stray_between_ends {
                press(&mut dom, sink, commit, Modifiers::empty());
            }
            compose(&mut dom, sink, "compositionend", "^");
            source_of(&dom)
        }

        let cut = "26-07-23";
        assert_eq!(cut_with(true, false), cut, "flagged commit: d^ cuts");
        assert_eq!(
            cut_with(false, false),
            cut,
            "unflagged commit: the stray keydown must not eat the d"
        );
        assert_eq!(
            cut_with(false, true),
            cut,
            "stray keydown between the two ends: same"
        );
    }

    #[test]
    fn a_normal_mode_composition_reaches_the_grammar() {
        // ^ is a dead key on a French layout, so it never arrives as a
        // keydown at all — only as a committed composition, which normal
        // mode used to discard whole
        // (adr/2026-08-normal-mode-compositions-reach-the-grammar.md)
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, sink) = activate_heading(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 4);
        let before = source_of(&dom);

        // the two ends WebKitGTK can fire that mean nothing
        compose(&mut dom, sink, "compositionend", "");
        assert_eq!(source_of(&dom), before, "the empty end is still inert");
        compose(&mut dom, sink, "compositionend", "ab");
        assert_eq!(
            source_of(&dom),
            before,
            "a multi-cluster commit is a real IME's, discarded whole"
        );

        // d then a committed ^ cuts back to the line's first non-blank
        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );
        compose(&mut dom, sink, "compositionend", "^");
        assert_eq!(
            source_of(&dom),
            "26-07-23",
            "d^ cut the heading back to its first non-blank"
        );

        // and it is one undo step like every other change intent
        // (adr/2026-08-undo-at-vim-grain.md)
        let undone = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("u".into()),
            Modifiers::empty(),
        );
        // the caret's own checkpoint here never dirtied the text either,
        // so undo falls back to the file-open checkpoint and its own last
        // block, the note's own trailing empty line
        // (adr/2026-08-cursor-always-in-the-note.md) — `clicks` is stale
        // by now, so the undo's own mutations are where the fresh click
        // target lives; the heading and the link block after it are the
        // only blocks whose byte range shifted back with the edit, so
        // they are the only ones remounted with a fresh listener, the
        // heading first (adr/2026-08-css-draws-the-markup.md)
        let heading = listeners(&undone, "click")[0];
        activate_block(&mut dom, heading);
        assert_eq!(source_of(&dom), before, "u puts the heading back");
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

        press(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
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

        press(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        // the heading is its own line-block now
        // (adr/2026-08-per-line-block-segmentation.md): from its end,
        // shift+up has nothing above inside the block, so it selects the
        // whole line instead of sliding
        press(&mut dom, sink, Key::ArrowUp, Modifiers::SHIFT);
        let html = dioxus_ssr::render(&dom);
        // the heading is the CSS verdict, so its `sel` spans each carry a
        // markup role too (adr/2026-08-css-draws-the-markup.md)
        assert!(
            html.contains(r#"class="sel mk-"#),
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
            "X",
            "the keystroke replaced the selection"
        );
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="sel mk-"#),
            "and collapsed it"
        );

        // tab moves the line a level in, shift+tab brings it back
        press(&mut dom, sink, Key::Tab, Modifiers::empty());
        assert_eq!(source_of(&dom), "  X", "the line took an indent level");
        press(&mut dom, sink, Key::Tab, Modifiers::SHIFT);
        assert_eq!(source_of(&dom), "X", "and gave it back");
        // a ctrl chord passes the grammar by, and the sink's own net
        // swallows it rather than letting focus walk out
        press(&mut dom, sink, Key::Tab, Modifiers::CONTROL);
        assert_eq!(source_of(&dom), "X", "ctrl+tab is inert");

        // the erase keys answer
        press(&mut dom, sink, Key::Delete, Modifiers::empty());
        assert_eq!(source_of(&dom), "X", "nothing to their right");
        press(&mut dom, sink, Key::Backspace, Modifiers::CONTROL);
        assert_eq!(source_of(&dom), "", "the word went");
        press(&mut dom, sink, Key::Backspace, Modifiers::empty());
        assert_eq!(source_of(&dom), "", "nothing left to take");
    }

    // -- the active block wears the same markup roles as an inactive one
    //    (adr/2026-08-css-draws-the-markup.md) ---------------------------

    /// The active block draws its own structural role on `.block-active`
    /// and its own `HeadingMarker` as a `mk-marker` span, exactly what an
    /// inactive `Pane::Css` heading would draw — entering the block never
    /// drops its markup.
    #[test]
    fn an_active_heading_carries_its_block_class_and_a_marker_span() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-23.typ"),
            linking(time_note("2026-07-23", "daily"), "2026-07-22")
                .replace("= 2026-07-23", "= Titre"),
        )
        .expect("the day note is overwritten with a plain heading");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        activate_heading(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="block-active mk-h1""#),
            "the heading's own structural role rides the active container: {html}"
        );
        assert!(
            html.contains(
                r#"<span class="mk-marker" data-start="0">= </span>"#
            ),
            "the heading marker keeps its role while active: {html}"
        );
    }

    /// A `Strong` run's own two delimiter bytes and its interior text stay
    /// tagged `mk-strong` (the delimiters additionally `mk-delim`) while the
    /// block is the active widget, and the caret drawn over it is still
    /// exactly one span — the markup split never doubles the caret up.
    #[test]
    fn an_active_strong_run_carries_its_role_over_both_delimiters_with_one_caret()
     {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-23.typ"),
            linking(time_note("2026-07-23", "daily"), "2026-07-22")
                .replace("= 2026-07-23", "*gras*"),
        )
        .expect("the day note is overwritten with a strong run");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // clicking activates the block at its own end, past the closing
        // delimiter, so the caret never lands on a byte a markup boundary
        // also claims
        activate_heading(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert_eq!(
            html.matches(r#"class="mk-strong mk-delim""#).count(),
            2,
            "both delimiter bytes keep the strong role, flagged: {html}"
        );
        assert_eq!(
            html.matches(r#"class="mk-strong""#).count(),
            1,
            "the interior run keeps the strong role, unflagged: {html}"
        );
        assert_eq!(
            html.matches("class=\"caret").count(),
            1,
            "the markup split still draws exactly one caret: {html}"
        );
    }

    /// A block under a visual selection wears the same roles as an inactive
    /// one: a covered checklist item still carries its `Checkbox` role
    /// alongside the `sel` highlight.
    #[test]
    fn a_selected_checklist_item_carries_its_checkbox_role_and_sel() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-23.typ"),
            format!(
                "{}- [x] fait\nafter\n",
                linking(time_note("2026-07-23", "daily"), "2026-07-22")
            ),
        )
        .expect("the day note is overwritten with a checklist line");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("0".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        let mut sink = sink;
        // three j's: past the link line and onto the checklist item, then
        // off it again onto "after" — the checklist line is covered but
        // never the active widget
        for _ in 0..3 {
            let woken = press_for_mutations(
                &mut dom,
                sink,
                Key::Character("j".into()),
                Modifiers::empty(),
            );
            sink = listeners(&woken, "keydown")
                .first()
                .copied()
                .unwrap_or(sink);
        }
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="sel mk-checkbox mk-checkbox-done""#),
            "the covered checkbox keeps its done role alongside sel: {html}"
        );
        // the container itself carries the item's block role too, the same
        // way .block-active and an inactive .block-css do, so a covered
        // list item keeps its left padding/indent under selection
        // (adr/2026-08-css-draws-the-markup.md)
        assert!(
            html.contains(r#"class="block-selected mk-item""#),
            "the covered block keeps its list-item role class: {html}"
        );
    }

    /// A standalone block whose own top-level list item is itself indented
    /// (no continuing parent in the same block — `blocks::segment` cut a
    /// fresh block at the blank line before it) steps its indent out
    /// through `--mk-indent`, instead of `block_class` flattening every
    /// depth to the same `mk-item` class
    /// (adr/2026-08-css-draws-the-markup.md). A contiguous parent/child
    /// pair sharing one block instead relies on the child's own preserved
    /// leading whitespace, which an SSR string cannot distinguish from the
    /// collapsed-whitespace bug it fixes — that half is judged in the
    /// running app, per the acceptance criteria.
    #[test]
    fn an_inactive_standalone_item_steps_its_indent_out() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-23.typ"),
            format!(
                "{}top\n\n  - deep\nafter\n",
                linking(time_note("2026-07-23", "daily"), "2026-07-22")
            ),
        )
        .expect("the day note is overwritten with a standalone deep item");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // moves the active widget onto the heading, so the list line
        // renders inactive (Pane::Css), not the active or selected path
        activate_heading(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(
                r#"class="block block-css mk-item " style="--mk-indent: 1">"#
            ),
            "the standalone deep item steps its indent out: {html}"
        );
    }

    /// A block CSS cannot draw (an equation) stays unstyled even as the
    /// active widget: no markup class anywhere, and the caret drawn over it
    /// is untouched by the fallback.
    #[test]
    fn an_active_equation_falls_back_to_unstyled_pieces_with_the_caret_intact()
    {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-23.typ"),
            linking(time_note("2026-07-23", "daily"), "2026-07-22")
                .replace("= 2026-07-23", "$x^2$"),
        )
        .expect("the day note is overwritten with an equation heading");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        activate_heading(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="block-active ""#),
            "the Typst verdict adds no second class: {html}"
        );
        assert!(
            html.contains(r#"<span data-start="0">$x^2$</span>"#),
            "the equation's own bytes reach the DOM with no markup role: {html}"
        );
        assert_eq!(
            html.matches("class=\"caret").count(),
            1,
            "the fallback still draws exactly one caret: {html}"
        );
    }

    #[test]
    fn v_paints_whole_lines_and_lowercase_v_stays_byte_exact() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // the preamble is the one block that still spans several lines —
        // import, show and meta merge
        // (adr/2026-08-per-line-block-segmentation.md)
        let (_, sink) = activate_preamble(&mut dom, &clicks);

        // k off the meta line onto "#show: note"; 0 l l starts the run at
        // column 2 — off column 0, where a charwise span from the same
        // anchor would be indistinguishable from a widened one and this
        // test would pass with `linewise` hardcoded false
        for key in "k0ll".chars() {
            press(
                &mut dom,
                sink,
                Key::Character(key.to_string()),
                Modifiers::empty(),
            );
        }

        // V j: whole-line visual, extended down onto the meta line
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        // the preamble (import + show + meta merged into one block,
        // adr/2026-08-per-line-block-segmentation.md) is the Typst
        // verdict — no markup role to add, so the merged `class` attribute
        // carries a trailing space where the role would otherwise sit
        // (adr/2026-08-css-draws-the-markup.md)
        assert!(
            html.contains(
                r#"<span class="sel " data-start="37">#show: note</span>"#
            ),
            "the show line paints whole, from before the anchor: {html}"
        );
        assert!(
            html.contains(r#"class="sel " data-start="52""#),
            "the meta line paints past the caret's own cluster: {html}"
        );

        // escape back to normal, up onto the show line again, then the
        // same j through charwise v: nothing past the caret enters a sel
        // piece — v stays byte-exact
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        press(
            &mut dom,
            sink,
            Key::Character("k".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        let charwise_html = dioxus_ssr::render(&dom);
        assert!(
            !charwise_html.contains(r#"class="sel " data-start="52""#),
            "v paints nothing past the head: {charwise_html}"
        );
    }

    /// V j crossing from the heading into the link line below it: the
    /// heading is no longer the active block, but the selection still
    /// reaches into it, so it splits out of the compiled region into its
    /// own highlighted `Selected` pane instead of folding back into an SVG
    /// fragment — two distinct source lines, each carrying its own `sel`
    /// spans (adr/2026-08-visual-selection-drawn-across-lines.md).
    #[test]
    fn capital_v_then_j_highlights_the_line_it_leaves_and_the_line_it_enters()
    {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        // 0: the anchor at the heading's own start, so V widens the whole
        // line rather than the degenerate empty span an end-of-line anchor
        // would leave in it
        press(
            &mut dom,
            sink,
            Key::Character("0".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);

        assert!(
            html.contains(r#"class="block-selected mk-h1""#)
                && html.contains(r#"class="selected-source""#),
            "the heading split out of its compiled region, its block role \
             class intact under selection just as it is active or inactive \
             (adr/2026-08-css-draws-the-markup.md): {html}"
        );
        // the covered heading's own markup roles tile it the same way the
        // active-pane test above does (adr/2026-08-css-draws-the-markup.md)
        assert!(
            html.contains(
                r#"<span class="sel mk-marker" data-start="0">= </span>"#
            ) && html.contains(
                r#"<span class="sel mk-text" data-start="2">2026-07-23</span>"#
            ),
            "the heading paints whole, as a covered but inactive line: {html}"
        );
        // the link line is now the active widget; V still widens its own
        // rendering the way the single-block case already did
        // (adr/2026-08-v-highlight-covers-whole-lines.md) — proof the
        // active pane itself carries a second, distinct sel line
        let active = html
            .split(r#"class="block-active"#)
            .nth(1)
            .unwrap_or_default();
        assert!(
            active.contains(r#"class="sel mk-link""#),
            "the line entered also carries a highlight: {html}"
        );
    }

    /// A covered block CSS cannot draw falls back the same way an inactive
    /// one would: the `Selected` pane's own Typst arm runs, so its pieces
    /// carry no markup class at all — only the `sel` highlight, drawn
    /// exactly as the merged-class Typst case above draws it.
    #[test]
    fn a_selected_block_that_falls_back_to_typst_carries_no_markup_role() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-23.typ"),
            format!(
                "{}\n$x^2$\nafter\n",
                linking(time_note("2026-07-23", "daily"), "2026-07-22")
            ),
        )
        .expect("the day note is overwritten with an equation line");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("0".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        let mut sink = sink;
        // four j's: past the link line and the blank line, over the
        // equation, and onto "after" — the equation is now fully covered
        // but never the active widget
        for _ in 0..4 {
            let woken = press_for_mutations(
                &mut dom,
                sink,
                Key::Character("j".into()),
                Modifiers::empty(),
            );
            sink = listeners(&woken, "keydown")
                .first()
                .copied()
                .unwrap_or(sink);
        }
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="sel " data-start="0">$x^2$</span>"#),
            "the covered equation keeps its highlight with no markup role: {html}"
        );
    }

    /// A `Selected` pane is still a block: clicking it activates it and
    /// sweeps the fragment cache exactly as clicking a compiled `Fragment`
    /// or `Pending` pane does — the highlight is drawn differently, not the
    /// click (adr/2026-08-visual-selection-drawn-across-lines.md).
    #[test]
    fn clicking_a_selected_pane_activates_its_block() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("0".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        let crossed = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        // the only click this crossing mounts: the heading's own now-
        // `Selected` pane
        let target = listeners(&crossed, "click")[0];
        click(&mut dom, target);
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains(r#"class="block-selected""#),
            "the pane is no longer drawn as covered source: {html}"
        );
        assert!(
            html.split(r#"class="block-active"#)
                .nth(1)
                .unwrap_or_default()
                .contains("2026-07-23"),
            "the click activated the heading: {html}"
        );
    }

    /// The mirror of the heading-crossing tests above: the selection's far
    /// end (the anchor, held since the note's own trailing empty line) sits
    /// *below* the active block once the caret walks back up past it, so
    /// the blocks in between split into `Selected` panes while the note's
    /// own last line — past the anchor, never covered — still compiles.
    #[test]
    fn a_selection_reaching_upward_leaves_a_compiled_region_below_the_far_block()
     {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // the note opens with its own trailing empty line active
        // (adr/2026-08-cursor-always-in-the-note.md) — the anchor V leaves
        // there
        let (_, sink) = activate_link(&mut dom, &clicks);
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        let sink = listeners(&woken, "keydown")
            .first()
            .copied()
            .unwrap_or(sink);
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        // two ups: past the link line and onto the heading, the anchor left
        // two blocks behind on the note's own last line
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("k".into()),
            Modifiers::empty(),
        );
        let sink = listeners(&woken, "keydown")
            .first()
            .copied()
            .unwrap_or(sink);
        press(
            &mut dom,
            sink,
            Key::Character("k".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);

        assert!(
            html.contains(r#"class="block-selected mk-line""#),
            "the link line split out of its compiled region, its Plain \
             block role class intact under selection \
             (adr/2026-08-css-draws-the-markup.md): {html}"
        );
        assert!(
            html.split(r#"class="block-active"#)
                .nth(1)
                .unwrap_or_default()
                .contains("2026-07-23"),
            "the heading is now the widget: {html}"
        );
        // the trailing empty line, past the anchor, is never covered — it
        // renders through the markup model rather than joining the
        // `Selected` run: exactly one covered block (the link line), the
        // blank line past it drawn as CSS, never as compiled Typst — a
        // blank block never needs the fallback whatever the selection
        // (adr/2026-08-css-draws-the-markup.md)
        assert_eq!(
            html.matches(r#"class="block-selected mk-line""#).count(),
            1,
            "only the link line is covered: {html}"
        );
        assert!(
            html.contains("mk-blank"),
            "the trailing blank line past the anchor still renders: {html}"
        );
    }

    /// The charwise mirror: v never widens, so a motion crossing into the
    /// next block leaves both the line it left and the line it entered only
    /// partly covered — the accepted friction
    /// `adr/2026-08-visual-selection-is-the-anchor.md` named up front.
    #[test]
    fn v_then_a_crossing_motion_highlights_both_lines_only_partly() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        // three columns in from the heading's own start, so the covered
        // slice is provably short of the whole line
        for key in "0lll".chars() {
            press(
                &mut dom,
                sink,
                Key::Character(key.to_string()),
                Modifiers::empty(),
            );
        }
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);

        let heading = html
            .split(r#"class="selected-source""#)
            .nth(1)
            .and_then(|rest| rest.split(r#"class="block-active"#).next())
            .unwrap_or_default();
        // the heading is the CSS verdict, so its covered slice carries a
        // markup role alongside `sel` (adr/2026-08-css-draws-the-markup.md)
        assert!(
            heading.contains(r#"class="sel mk-"#),
            "the heading carries a highlight: {html}"
        );
        assert!(
            heading.contains(r#"data-start="0">"#)
                && !heading
                    .split(r#"data-start="0">"#)
                    .nth(1)
                    .unwrap_or_default()
                    .starts_with("= 2026-07-23</span>"),
            "the heading's own first bytes stay plain: v never widens \
             a line it only partly covers: {heading}"
        );
        let active = html
            .split(r#"class="block-active"#)
            .nth(1)
            .unwrap_or_default();
        assert!(
            active.contains(r#"class="sel mk-link""#),
            "the line entered carries a highlight too: {html}"
        );
    }

    /// d over a V-widened selection that crosses out of the active block:
    /// the operator still takes the true note-global span — the heading and
    /// the link line both go — even though only the link line was ever the
    /// textarea (adr/2026-08-visual-selection-drawn-across-lines.md,
    /// adr/2026-08-visual-selection-is-the-anchor.md).
    #[test]
    fn d_over_a_selection_crossing_blocks_deletes_the_whole_span() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("0".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        // the crossing mounts a fresh widget for the line entered — the
        // link line — and d must land on its own listener, not the
        // heading's now-stale one
        let crossed = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        let sink = listeners(&crossed, "keydown")
            .first()
            .copied()
            .unwrap_or(sink);
        press(
            &mut dom,
            sink,
            Key::Character("d".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));

        let file = vault.path().join("time/2026-07-23.typ");
        let saved =
            std::fs::read_to_string(&file).expect("the note is readable");
        assert!(!saved.contains("= 2026-07-23"), "the heading went: {saved}");
        assert!(
            !saved.contains("2026-07-22"),
            "the link line went too: {saved}"
        );
        assert!(saved.contains("#meta"), "the preamble survives: {saved}");
    }

    /// Escape drops the mode, collapsing the caret and clearing the
    /// selection: the covered line goes back to being a compiled region
    /// rather than staying stuck as highlighted source
    /// (adr/2026-08-visual-selection-drawn-across-lines.md).
    #[test]
    fn leaving_visual_mode_clears_every_highlight() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("0".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("V".into()),
            Modifiers::empty(),
        );
        let crossed = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="block-selected mk-h1""#), "{html}");

        // the crossing mounted a fresh widget for the link line; its own
        // keydown listener is what Escape must land on now
        let sink = listeners(&crossed, "keydown")
            .first()
            .copied()
            .unwrap_or(sink);
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains(r#"class="block-selected""#),
            "no pane stays selected: {html}"
        );
        assert!(
            !html.contains(r#"class="sel""#),
            "no highlight survives: {html}"
        );
        assert!(
            html.contains(r#"class="block-active"#),
            "one widget still stands: {html}"
        );
    }

    /// A count on plain j, resolved through the logical-line fallback
    /// (`walk_visual_fallback`): 2j must land exactly where two separate
    /// j presses do, content-independent proof that the fallback still
    /// composes the count
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_count_on_plain_j_lands_where_two_separate_j_presses_do() {
        fn rendered_after(second: &str) -> String {
            let vault = temp_vault();
            let (mut dom, clicks, _, _) =
                rendered_app(Some(vault.path().to_path_buf()));
            let (_, sink) = activate_heading(&mut dom, &clicks);
            // gg: a fixed, deterministic starting line for both variants
            press(
                &mut dom,
                sink,
                Key::Character("g".into()),
                Modifiers::empty(),
            );
            let woken = press_for_mutations(
                &mut dom,
                sink,
                Key::Character("g".into()),
                Modifiers::empty(),
            );
            let sink = listeners(&woken, "keydown")[0];
            for key in second.chars() {
                press(
                    &mut dom,
                    sink,
                    Key::Character(key.to_string()),
                    Modifiers::empty(),
                );
            }
            dioxus_ssr::render(&dom)
        }

        assert_eq!(
            rendered_after("2j"),
            rendered_after("jj"),
            "2j lands where j j does, through the fallback"
        );
    }

    /// The one state a real keystroke can never produce: `apply_vim` only
    /// ever runs once the sink has already matched an open note against a
    /// caret, so `walk_visual_fallback`'s "nothing to land on" guard is
    /// proven here directly, over a closed editor, rather than through a
    /// keystroke that cannot reach it. Nothing moves and no goal column
    /// comes back — and `bounded_steps` reads the same closed editor, so
    /// the same state proves its own "nothing to walk" answer.
    #[test]
    fn walk_visual_fallback_does_nothing_without_an_open_note() {
        #[component]
        fn Probe() -> Element {
            let editor = use_signal(Editor::closed);
            let step = Step {
                down: true,
                count: 1,
                extend: false,
                held: Goal::default(),
            };
            let goal = walk_visual_fallback(editor, step, 1, Some(4));
            let snapshot = editor.read();
            let untouched = goal.is_none()
                && snapshot.note().is_none()
                && snapshot.caret().is_none()
                && snapshot.active().is_none();
            rsx! { "{untouched}" }
        }
        let mut dom = VirtualDom::new(Probe);
        dom.rebuild_to_vec();
        assert_eq!(
            dioxus_ssr::render(&dom),
            "true",
            "a closed editor walks nowhere and remembers no column"
        );
        assert_eq!(
            bounded_steps(&Editor::closed(), 9),
            1,
            "no note open, nothing to walk: one degraded step"
        );
    }

    /// A scripted landing places the caret exactly where the seam says,
    /// not where the logical fallback would have
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_scripted_line_probe_answer_places_the_caret_at_that_block_offset() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, _) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // the seam's own answer: block-relative byte 0, no UTF-16 offset
        // into it — the caret lands at the block's very start, which
        // `activate`'s own end-of-block landing never puts it at
        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 12.0)];
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="caret-box" data-start="0""#),
            "the scripted landing wins over the fallback: {html}"
        );
    }

    /// A miss the seam reports is indistinguishable from no seam at all:
    /// both take the exact same logical-line step
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_scripted_miss_and_no_probe_at_all_land_on_the_same_logical_line() {
        let vault = temp_vault();

        let (mut dom_absent, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom_absent, &clicks);
        press(
            &mut dom_absent,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom_absent));

        let (mut dom_miss, clicks, drawn, _) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom_miss, &clicks);
        drawn.lock().expect("the line cell never poisons").clear();
        press(
            &mut dom_miss,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom_miss));

        assert_eq!(
            dioxus_ssr::render(&dom_absent),
            dioxus_ssr::render(&dom_miss),
            "a reported miss and an absent seam degrade identically"
        );
    }

    /// `3j` is one grammar act and one round trip: the whole count crosses
    /// the seam at once, and the caret lands on the run's *third* drawn
    /// line, not its first. A step-per-eval walk could not do this — the
    /// second eval would be built before the first landing's DOM edits
    /// flushed and would read the caret's pre-move rect
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_count_of_three_walks_three_drawn_lines_in_one_round_trip() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, asked) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // three drawn lines below the caret, each a different block offset
        *drawn.lock().expect("the line cell never poisons") =
            vec![(1, 0, 77.0), (3, 0, 77.0), (5, 0, 77.0)];
        press(
            &mut dom,
            sink,
            Key::Character("3".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));

        assert_eq!(
            *asked.lock().expect("the goal cell never poisons"),
            vec![(None, 3)],
            "one ask carries the whole count"
        );
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="caret-box" data-start="5""#),
            "the caret sits on the third drawn line, not the first: {html}"
        );
    }

    /// A count the drawn lines cannot fill: the seam walks what it has and
    /// says so, and the rest of the run goes over the logical fallback —
    /// which is what leaves the active block
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_count_past_the_drawn_extent_finishes_through_the_fallback() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, _) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // one drawn line above the caret, then nothing: 3k takes the one
        // step the seam has and walks the other two logically, out of the
        // heading block and into the preamble above it
        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 9.0)];
        press(
            &mut dom,
            sink,
            Key::Character("3".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("k".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));

        assert!(
            source_of(&dom).contains("#import"),
            "the run finished logically into the preamble: {}",
            source_of(&dom)
        );
    }

    /// The count is clamped to the note's drawn extent before anything
    /// walks, so an absurd one asks the webview for a walk it can finish
    /// rather than four billion steps (CLAUDE.md: bounded loops).
    #[test]
    fn an_absurd_count_is_clamped_to_the_notes_drawn_extent() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, asked) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 9.0)];
        for key in "999999999j".chars() {
            press(
                &mut dom,
                sink,
                Key::Character(key.to_string()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));

        let asked_for = asked
            .lock()
            .expect("the goal cell never poisons")
            .first()
            .map(|(_, count)| *count);
        let note = std::fs::read_to_string(
            vault.path().join("time").join(format!("{TODAY}.typ")),
        )
        .expect("the fixture note is on disk");
        assert!(
            asked_for.is_some_and(|count| count <= note.chars().count() + 1),
            "the walk was clamped to the note's characters: {asked_for:?}"
        );
    }

    /// A key the grammar swallows is still another key, so it forgets the
    /// run's goal column too. Without this the ADR's "cleared by every
    /// other key" would hold only for keys that produce acts
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_run_broken_by_a_swallowed_key_forgets_the_goal_x() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, asked) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 31.0)];
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        // "!" is bound to nothing: the grammar consumes it and runs
        // nothing, so it never reaches `apply_vim` at all
        press(
            &mut dom,
            sink,
            Key::Character("!".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));

        assert_eq!(
            asked
                .lock()
                .expect("the goal cell never poisons")
                .last()
                .copied(),
            Some((None, 1)),
            "the swallowed key between the two runs forgot the goal x"
        );
    }

    /// A key pressed while a walk is still awaiting its probe forgets the
    /// run synchronously; the walk then resolves into a run that no longer
    /// exists, and must write neither its goal column nor its landing —
    /// the key that forgot the run has already moved the caret, and a late
    /// landing would yank it back
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md). Nothing
    /// settles between the two presses, which is the only way to have a
    /// walk in flight when the next key lands.
    #[test]
    fn a_walk_resolving_after_its_run_was_forgotten_moves_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, asked, release) =
            latched_line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        retype(&mut dom, sink, "abcdefgh\nij\nklmnopqr");
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        block_on(settle(&mut dom));
        // column 5 of the last line: far from the landing the seam is
        // about to answer, so a late write is visible as a caret jump
        for key in "0lllll".chars() {
            press(
                &mut dom,
                sink,
                Key::Character(key.to_string()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));

        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 64.0)];
        // j sends the walk out and it hangs there; h lands while it is
        // still out
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("h".into()),
            Modifiers::empty(),
        );
        // now the walk resolves, into the run h already forgot
        release.send(()).expect("the walk is still waiting");
        block_on(settle(&mut dom));

        let rendered = dioxus_ssr::render(&dom);
        assert!(
            rendered.contains(r#"class="caret-box" data-start="16""#),
            "the late landing left the caret where h put it: {rendered}"
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));

        assert_eq!(
            asked
                .lock()
                .expect("the goal cell never poisons")
                .last()
                .copied(),
            Some((None, 1)),
            "the late landing did not resurrect the forgotten goal x"
        );
    }

    /// A count is not "another key": vim's curswant survives one, so the
    /// 2 of a 2j walks from the column the j before it resolved rather
    /// than bootstrapping a fresh one from where the caret now stands
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_count_between_two_runs_keeps_the_goal_x() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, asked) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 42.0)];
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        for key in "2j".chars() {
            press(
                &mut dom,
                sink,
                Key::Character(key.to_string()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));

        assert_eq!(
            asked
                .lock()
                .expect("the goal cell never poisons")
                .last()
                .copied(),
            Some((Some(42.0), 2)),
            "the count digit held the first run's goal x"
        );
    }

    /// Every mouse-driven caret move ends a j/k run, the way
    /// `Editor::place_in_block` drops the editor's own logical goal: a
    /// press, a drag and a click onto another block all forget the pixel
    /// column, so a j after clicking elsewhere does not walk back to the
    /// pre-click column
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn the_mouse_forgets_a_walks_goal_column() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, asked, hit) =
            line_and_hit_app(Some(vault.path().to_path_buf()));
        let (block, sink) = activate_heading(&mut dom, &clicks);
        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 88.0)];
        *hit.lock().expect("the hit cell never poisons") = Some((0, 1));

        // each mouse gesture in turn, every one of them between two runs
        for gesture in ["mousedown", "mousemove"] {
            press(
                &mut dom,
                sink,
                Key::Character("j".into()),
                Modifiers::empty(),
            );
            block_on(settle(&mut dom));
            mouse(&mut dom, gesture, block, (0.0, 0.0));
            block_on(settle(&mut dom));
            press(
                &mut dom,
                sink,
                Key::Character("j".into()),
                Modifiers::empty(),
            );
            block_on(settle(&mut dom));
            assert_eq!(
                asked
                    .lock()
                    .expect("the goal cell never poisons")
                    .last()
                    .copied(),
                Some((None, 1)),
                "{gesture} between two runs forgot the goal x"
            );
        }

        // and the click that activates another block, which moves the
        // caret without any probe at all: the blank line directly
        // (adr/2026-08-css-draws-the-markup.md)
        mouse(&mut dom, "mouseup", block, (0.0, 0.0));
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let landed = click_for_mutations(&mut dom, clicks[BLOCK_BLANK]);
        let blank_keys = listeners(&landed, "keydown")[0];
        let activated = press_for_mutations(
            &mut dom,
            blank_keys,
            Key::ArrowUp,
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(
            source_of(&dom).contains("#import"),
            "the preamble block is the active one now: {}",
            source_of(&dom)
        );
        let sink = listeners(&activated, "keydown")[0];
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert_eq!(
            asked
                .lock()
                .expect("the goal cell never poisons")
                .last()
                .copied(),
            Some((None, 1)),
            "activating another block forgot the goal x too"
        );
    }

    /// The logical fallback holds the run's goal column just as the seam
    /// holds its pixel x. This is the common path — every block crossing
    /// and every note edge takes it — and since the grammar handed the
    /// whole walk to the executor, the executor is the only thing left
    /// that can remember: a k over a short line must not forget where the
    /// run started, and the k after it lands back on the original column
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_fallback_run_over_a_short_line_keeps_its_goal_column() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        // three lines in one block, the middle one too short to hold the
        // column the run starts at
        retype(&mut dom, sink, "abcdefgh\nij\nklmnopqr");
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        block_on(settle(&mut dom));

        // column 5 of the last line, then k k
        for key in "0lllll".chars() {
            press(
                &mut dom,
                sink,
                Key::Character(key.to_string()),
                Modifiers::empty(),
            );
        }
        press(
            &mut dom,
            sink,
            Key::Character("k".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let clamped = dioxus_ssr::render(&dom);
        assert!(
            clamped.contains(r#"class="caret-box" data-start="10""#),
            "the short line clamps the walk onto its last cluster: {clamped}"
        );

        press(
            &mut dom,
            sink,
            Key::Character("k".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let restored = dioxus_ssr::render(&dom);
        assert!(
            restored.contains(r#"class="caret-box" data-start="5""#),
            "the run remembered column 5 through the short line: {restored}"
        );
    }

    /// The goal x lives only across a j/k run: any other key — even one
    /// the grammar accepts — forgets it, so the next run bootstraps fresh
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn a_run_broken_by_h_forgets_the_goal_x() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, asked) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        *drawn.lock().expect("the line cell never poisons") =
            vec![(0, 0, 55.0)];
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        press(
            &mut dom,
            sink,
            Key::Character("h".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));

        assert_eq!(
            asked
                .lock()
                .expect("the goal cell never poisons")
                .last()
                .copied(),
            Some((None, 1)),
            "h between two j runs forgets the held goal x"
        );
    }

    /// Visual mode's j extends the selection to the seam's landing instead
    /// of collapsing the caret onto it
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    #[test]
    fn visual_mode_line_probe_j_extends_instead_of_placing() {
        let vault = temp_vault();
        let (mut dom, clicks, drawn, _) =
            line_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // v anchors where activate() left the caret — the block's end —
        // so a place (rather than an extend) would collapse the anchor
        // there too and leave no selection at all
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::empty(),
        );
        *drawn.lock().expect("the line cell never poisons") =
            vec![(5, 0, 12.0)];
        press(
            &mut dom,
            sink,
            Key::Character("j".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="caret-box" data-start="5""#),
            "the head moved to the scripted landing: {html}"
        );
        assert!(
            html.contains(r#"class="sel mk-"#),
            "the anchor held instead of collapsing onto the head: {html}"
        );
    }

    #[test]
    fn the_clipboard_chords_round_trip_through_the_seams() {
        let vault = temp_vault();
        let (mut dom, clicks, written) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Ok("collé".to_string()),
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
            vec!["= 2026-07-23".to_string()],
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

        // a paste whose read fails leaves the note alone and reports above
        let (mut dom, clicks, _) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Err("read denied".to_string()),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("clipboard: read denied — no text was read"),
            "{html}"
        );
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
        // "= 202" tiles across the heading's own markup roles — the marker
        // with its space, then the digits, each keep their own span even
        // while every one of them is selected
        // (adr/2026-08-css-draws-the-markup.md)
        fn drags_across_the_prefix(html: &str) -> bool {
            html.contains(
                r#"<span class="sel mk-marker" data-start="0">= </span>"#,
            ) && html.contains(
                r#"<span class="sel mk-text" data-start="2">202</span>"#,
            )
        }
        let html = dioxus_ssr::render(&dom);
        assert!(
            drags_across_the_prefix(&html),
            "the drag drew the selection: {html}"
        );

        // a move whose probe misses extends nothing
        *hit.lock().expect("the hit cell never poisons") = None;
        mouse(&mut dom, "mousemove", block, (60.0, 0.0));
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            drags_across_the_prefix(&html),
            "the miss changed nothing: {html}"
        );

        // after the release, moves stop extending
        mouse(&mut dom, "mouseup", block, (41.0, 0.0));
        *hit.lock().expect("the hit cell never poisons") = Some((0, 9));
        mouse(&mut dom, "mousemove", block, (80.0, 0.0));
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            drags_across_the_prefix(&html),
            "the selection held where the button went up: {html}"
        );

        // one probe in flight: while one hangs, the next move is skipped
        // rather than queued
        mouse(&mut dom, "mousedown", block, (0.0, 0.0));
        *hit.lock().expect("the hit cell never poisons") = Some(HIT_HANGS);
        mouse(&mut dom, "mousemove", block, (90.0, 0.0));
        *hit.lock().expect("the hit cell never poisons") = Some((0, 9));
        mouse(&mut dom, "mousemove", block, (91.0, 0.0));
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains(r#"<span class="sel mk-text" data-start="6">"#),
            "the second move was skipped while the first probe was out: {html}"
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

    #[test]
    fn the_sink_takes_focus_back_when_the_settings_overlay_closes() {
        // regression: `settings_open` (and `notices_open`) were missing
        // from the focus effect's overlay set, so closing the overlay left
        // the focus stranded on <body> — a dead keyboard, with the typed
        // keystrokes replaying against the widget on the next click
        let vault = temp_vault();
        let (mut dom, mutations) =
            mounted_app(Some(vault.path().to_path_buf()), None);
        let keys = listeners(&mutations, "keydown");
        let focused = mount_counting_focus(
            &mut dom,
            listeners(&mutations, "mounted")[1],
        );
        block_on(settle(&mut dom));
        let before = focused.load(Ordering::SeqCst);

        let (overlay_keys, _) =
            open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        press(&mut dom, overlay_keys, Key::Escape, Modifiers::empty());
        block_on(settle(&mut dom));
        assert!(
            focused.load(Ordering::SeqCst) > before,
            "the sink asked for focus once the settings overlay closed"
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
    fn the_header_buttons_carry_hover_tooltips() {
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"title="previous month""#), "{html}");
        assert!(html.contains(r#"title="open today&#39;s note""#), "{html}");
        assert!(html.contains(r#"title="next month""#), "{html}");
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

    #[test]
    fn a_sabotaged_notes_table_fails_the_survey_and_the_count() {
        let vault = temp_vault();
        let index = sabotaged_index(vault.path(), "DROP TABLE notes");
        let error = survey(&index, test_today()).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
        let error = open_loops(&index, test_today()).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_sabotaged_links_table_fails_the_survey_count() {
        // the time-note half survives on the notes table; the count is what
        // reaches the links table and fails
        let vault = temp_vault();
        let index = sabotaged_index(vault.path(), "DROP TABLE links");
        let error = survey(&index, test_today()).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_vanished_anomalies_table_fails_only_the_loops_fourth_leg() {
        // the one sabotage the three earlier loops queries survive: only
        // the anomalies read fails (adr/2026-08-anomalies-join-the-loops.md)
        let vault = temp_vault();
        let index = sabotaged_index(vault.path(), "DROP TABLE anomalies");
        assert!(index.typeless_notes().is_ok());
        assert!(index.dangling_links().is_ok());
        assert!(index.unsummarized_captures().is_ok());
        assert!(index.due_notes(test_today()).is_ok());
        let error = open_loops(&index, test_today()).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_dropped_due_column_fails_only_the_loops_last_leg() {
        // the one sabotage every other loops query survives: the due read
        // is the fifth and last leg (adr/2026-09-course-type-and-due-loops.md)
        let vault = temp_vault();
        let index =
            sabotaged_index(vault.path(), "ALTER TABLE notes DROP COLUMN due");
        assert!(index.typeless_notes().is_ok());
        assert!(index.dangling_links().is_ok());
        assert!(index.unsummarized_captures().is_ok());
        assert!(index.anomalies().is_ok());
        let error = open_loops(&index, test_today()).unwrap_err();
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
        assert!(open_loops(&index, test_today()).is_ok());
        let error = survey(&index, test_today()).unwrap_err();
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
        assert!(open_loops(&index, test_today()).is_ok());
        assert!(index.table_notes().is_ok());
        let error = survey(&index, test_today()).unwrap_err();
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
        let error = open_loops(&index, test_today()).unwrap_err();
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
        dom.insert_any_root_context(Box::new(VaultFeed {
            changes: Arc::new(Mutex::new(Some(receiver))),
            trouble: None,
        }));
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
        assert!(
            html.contains(r#"class="ember" title="open loops">1</span>"#),
            "{html}"
        );
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
        assert!(
            html.contains(r#"class="ember" title="open loops">1</span>"#),
            "{html}"
        );
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
        assert!(dioxus_ssr::render(&dom).contains("indexing the vault"));
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
        assert!(dioxus_ssr::render(&dom).contains("indexing the vault"));
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
        assert!(dioxus_ssr::render(&dom).contains("indexing the vault"));
    }

    #[test]
    fn a_watcher_that_stops_is_silent_and_ends_the_task() {
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        drop(sender);
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("rail-id"),
            "the screen stands, it just stops hearing about the vault"
        );
        // the glyph carries the state without interrupting the note
        assert!(html.contains("liveness-unwatched"), "{html}");
        assert!(!html.contains("no longer watched"), "{html}");
        assert!(!html.contains("notice-warning"), "{html}");
    }

    #[test]
    fn a_watcher_that_would_not_start_is_an_unwatched_vault_on_screen() {
        // main hands the start failure over as the feed's trouble instead
        // of stderr (adr/2026-08-status-surface-owns-notices.md)
        let vault = temp_vault();
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(Some(
            vault.path().to_path_buf(),
        ))));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(VaultFeed {
            changes: Arc::new(Mutex::new(None)),
            trouble: Some("inotify refused".to_string()),
        }));
        with_reactor(|| dom.rebuild_to_vec());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("liveness-unwatched"), "{html}");
        assert!(
            html.contains("the vault is not watched: inotify refused"),
            "{html}"
        );
    }

    #[test]
    fn a_failed_seeding_is_a_notice_on_screen() {
        // main hands the seed failure over like the watcher's start
        // failure (adr/2026-08-templates-seeded-from-embedded-fixtures.md)
        let vault = temp_vault();
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(Some(
            vault.path().to_path_buf(),
        ))));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(SeedTrouble(Some(
            "permission denied".to_string(),
        ))));
        with_reactor(|| dom.rebuild_to_vec());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("templates: permission denied"), "{html}");
    }

    #[test]
    fn a_clean_seeding_reports_nothing() {
        let vault = temp_vault();
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(Some(
            vault.path().to_path_buf(),
        ))));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(SeedTrouble(None)));
        with_reactor(|| dom.rebuild_to_vec());
        assert!(!dioxus_ssr::render(&dom).contains("templates:"));
    }

    #[test]
    fn the_palette_lists_the_templates_and_escape_closes_the_picker() {
        let vault = temp_vault();
        // a stray non-typ file is not a template
        std::fs::write(vault.path().join("templates/readme.md"), "notes")
            .expect("the stray file is written");
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, picker_keys) =
            open_template_picker(&mut dom, keys[LOGS_KEYS]);
        assert_eq!(
            picker_ids(&dom),
            [
                "capture", "concept", "daily", "seasonal", "template",
                "weekly"
            ]
        );

        // an overlay is up: the summoning chords refuse to stack another
        press(&mut dom, picker_keys, ctrl_p(), Modifiers::CONTROL);
        press(&mut dom, picker_keys, ctrl_n(), Modifiers::CONTROL);
        press(&mut dom, picker_keys, ctrl_l(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command…"), "{html}");
        assert!(!html.contains("link to…"), "{html}");

        // a query nothing matches leaves a message, and enter does nothing
        type_into(&mut dom, input, "xyzzy");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching template"), "{html}");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains("no matching template"));

        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("no matching template"), "{html}");
        assert!(picker_ids(&dom).is_empty(), "{html}");
    }

    #[test]
    fn a_template_opens_in_the_centre_pane_wearing_its_own_crumbs() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let before = dioxus_ssr::render(&dom);
        assert!(before.contains("links-footer"), "{before}");
        assert!(before.contains("captured today"), "{before}");

        let (_input, picker_keys) =
            open_template_picker(&mut dom, keys[LOGS_KEYS]);
        // the arrows move the highlight: down, down, up lands on the
        // second row — "concept"
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowUp, Modifiers::empty());
        let opened = press_for_mutations(
            &mut dom,
            picker_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"<span class="crumb">templates</span>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<span class="crumb">concept</span>"#),
            "{html}"
        );
        // the selected note's furniture leaves with the note
        assert!(!html.contains("links-footer"), "{html}");
        assert!(!html.contains("captured today"), "{html}");
        // the template's own source stands in the pane, placeholders and
        // all — the note's own trailing empty line opens active
        // (adr/2026-08-cursor-always-in-the-note.md); every block wakes on
        // its own click now (adr/2026-08-css-draws-the-markup.md) — the
        // preamble's own key survives from the note that was showing
        // before, so it needs no fresh listener here, leaving the blank
        // line above the heading as the first click and the heading the
        // second
        activate_block(&mut dom, listeners(&opened, "click")[1]);
        assert!(source_of(&dom).contains("{{title}}"), "{}", source_of(&dom));

        // escape hands the pane back to the selected note
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let back = dioxus_ssr::render(&dom);
        assert!(
            !back.contains(r#"<span class="crumb">templates</span>"#),
            "{back}"
        );
        assert!(back.contains("2026-07-23"), "{back}");
        assert!(back.contains("links-footer"), "{back}");
    }

    #[test]
    fn escaping_out_of_a_template_does_not_push_a_history_visit() {
        // regression: the Escape-from-template return routes through
        // `select` with the selection already standing, which must not
        // count as a visit (adr/2026-08-note-history-back.md) —
        // otherwise the switcher would land on the very note it is already
        // showing instead of walking back to the one real visit underneath.
        //
        // a self-referential entry (the note pointing at itself) is
        // invisible if the prior selection is the same note the test starts
        // on, so this first walks to a *distinct* prior note (23 -> 21):
        // a wrongly-pushed self-visit would satisfy one switch by landing
        // back on 21 (a no-op), while the correct behaviour walks past it
        // to the one true visit, landing on 23.
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        assert!(
            dioxus_ssr::render(&dom)
                .contains("cal-day has-note selected\">23")
        );

        click(&mut dom, clicks[RAIL_DAY_21]);
        let before = dioxus_ssr::render(&dom);
        assert!(
            before.contains("cal-day has-note selected\">21"),
            "{before}"
        );

        let (_input, picker_keys) =
            open_template_picker(&mut dom, keys[LOGS_KEYS]);
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let restored = dioxus_ssr::render(&dom);
        assert_eq!(
            before, restored,
            "escape hands the pane straight back to what stood before"
        );

        let (_input, _picker_keys, _) =
            open_switcher(&mut dom, keys[LOGS_KEYS]);
        assert_eq!(
            picker_ids(&dom),
            ["2026-07-23"],
            "no self-referential visit was pushed: the picker offers only \
             the one true prior visit"
        );
    }

    #[test]
    fn a_clicked_row_opens_its_template() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "edit template");
        let opened = press_for_mutations(
            &mut dom,
            palette_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        mount(&mut dom, listeners(&opened, "mounted")[0]);
        // the rows are the dispatch's only click listeners, in list order
        click(&mut dom, listeners(&opened, "click")[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"<span class="crumb">capture</span>"#),
            "{html}"
        );
    }

    #[test]
    fn an_edited_template_reaches_the_disk() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, picker_keys) =
            open_template_picker(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "daily");
        // the chosen template opens with its own trailing empty line
        // awake (adr/2026-08-cursor-always-in-the-note.md); every block
        // above it wakes on its own click now
        // (adr/2026-08-css-draws-the-markup.md) — the preamble's own
        // key survives from the note that was showing before, so the
        // blank line above the heading is the first click, the heading
        // itself the second
        let woken = press_for_mutations(
            &mut dom,
            picker_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let (_, sink) =
            activate_block(&mut dom, listeners(&woken, "click")[1]);
        retype(&mut dom, sink, "= le modèle refait");
        block_on(settle(&mut dom));
        let text =
            std::fs::read_to_string(vault.path().join("templates/daily.typ"))
                .expect("the template is readable");
        assert!(text.contains("le modèle refait"), "{text}");
        assert!(
            !text.contains("= {{id}}"),
            "the heading was replaced: {text}"
        );
    }

    #[test]
    fn a_buffer_that_will_not_flush_keeps_the_template_from_opening() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let file = vault.path().join("time/2026-07-23.typ");
        edit_behind(&file, "= repris dehors\n");
        retype(&mut dom, sink, "= à moi\n");
        block_on(settle(&mut dom));

        let (_input, picker_keys) = open_template_picker(&mut dom, sink);
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        // the flush refused: the conflict stands, the template stayed shut
        assert!(html.contains("changed on disk"), "{html}");
        assert!(
            !html.contains(r#"<span class="crumb">templates</span>"#),
            "{html}"
        );
        // the picker is still up, waiting on the conflict's resolution
        assert!(!picker_ids(&dom).is_empty(), "{html}");
    }

    #[test]
    fn a_missing_templates_directory_is_a_notice_not_a_crash() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_dir_all(vault.path().join("templates"))
            .expect("the directory is removed");
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "edit template");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("templates:"), "{html}");
        assert!(picker_ids(&dom).is_empty(), "{html}");
    }

    #[test]
    fn a_screen_switch_closes_the_template_picker() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_input, picker_keys) =
            open_template_picker(&mut dom, keys[LOGS_KEYS]);
        // the chord bubbles through the overlay to the pane: the screen
        // switches and the picker, its input about to unmount, closes
        let mutations = press_for_mutations(
            &mut dom,
            picker_keys,
            Key::Character("1".into()),
            Modifiers::CONTROL,
        );
        assert!(picker_ids(&dom).is_empty());
        let table_keys = listeners(&mutations, "keydown")[0];

        // and the mirror: the picker the table now hosts leaves the same
        // way (adr/2026-09-edit-template-reaches-the-logs-from-the-table.md)
        let (_input, picker_keys) = open_template_picker(&mut dom, table_keys);
        assert!(!picker_ids(&dom).is_empty());
        press(
            &mut dom,
            picker_keys,
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(picker_ids(&dom).is_empty(), "{html}");
        assert!(html.contains(r#"class="logs""#), "{html}");
    }

    /// The command is available on the table too, and its picker is
    /// rendered inside that branch as well as the logs' so it stands over
    /// either screen: a pick switches to the logs and opens the template
    /// in the centre pane, the one full-page surface the shared editor has
    /// (adr/2026-09-edit-template-reaches-the-logs-from-the-table.md).
    #[test]
    fn the_table_picks_a_template_and_lands_on_the_logs() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let mutations = press_for_mutations(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("1".into()),
            Modifiers::CONTROL,
        );
        let table_keys = listeners(&mutations, "keydown")[0];
        assert!(dioxus_ssr::render(&dom).contains(r#"class="table""#));

        let (_input, picker_keys) = open_template_picker(&mut dom, table_keys);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="table""#), "still the table: {html}");
        assert_eq!(
            picker_ids(&dom),
            [
                "capture", "concept", "daily", "seasonal", "template",
                "weekly"
            ]
        );

        // a key that beat the input's focus grab reaches the table pane
        // and is relayed into the picker's query, never read as a chord
        // there (adr/2026-09-overlay-keys-relay-before-focus-lands.md)
        press(
            &mut dom,
            table_keys,
            Key::Character("c".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"value="c""#), "{html}");
        assert_eq!(picker_ids(&dom), ["capture", "concept"]);

        // and Escape closes it where it stands, the logs' own gesture
        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(picker_ids(&dom).is_empty(), "{html}");
        assert!(html.contains(r#"class="table""#), "{html}");

        let (input, picker_keys) = open_template_picker(&mut dom, table_keys);
        type_into(&mut dom, input, "concept");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "the screen came: {html}");
        assert!(
            html.contains(r#"<span class="crumb">templates</span>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<span class="crumb">concept</span>"#),
            "{html}"
        );
    }

    /// A sheet holds the same one editor, so it gets `select`'s
    /// bookkeeping — onto the switcher's log, then closed — before the screen
    /// changes, the order the table's own Ctrl+D follows
    /// (adr/2026-09-edit-template-reaches-the-logs-from-the-table.md).
    #[test]
    fn a_template_picked_over_a_sheet_takes_the_sheet_with_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, table_keys) =
            table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        let (input, picker_keys) = open_template_picker(&mut dom, table_keys);
        type_into(&mut dom, input, "daily");
        let landed = press_for_mutations(
            &mut dom,
            picker_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"class="sheet""#), "sheet gone: {html}");
        assert!(html.contains(r#"class="logs""#), "the screen came: {html}");
        assert!(
            html.contains(r#"<span class="crumb">daily</span>"#),
            "{html}"
        );

        // the sheet went onto the visit log on its way out: Escape out of
        // a template lands on the logs selection, so the Ctrl+O switcher's
        // recent rows are the only way back to the card
        let logs_keys = listeners(&landed, "keydown")[0];
        let (_input, _picker_keys, _) = open_switcher(&mut dom, logs_keys);
        assert_eq!(picker_ids(&dom), ["alpha"]);
    }

    #[test]
    fn a_degraded_watcher_heals_when_a_batch_lands_again() {
        let vault = temp_vault();
        let (mut dom, _, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        assert!(
            dioxus_ssr::render(&dom).contains("liveness-watching"),
            "the taken receiver is the watching state"
        );

        // squat the database: the batch fails, and so does its rescan
        std::fs::remove_file(vault.path().join(".index/index.db"))
            .expect("the database is there to remove");
        std::fs::create_dir(vault.path().join(".index/index.db"))
            .expect("a directory squats the database path");
        feed_batch(&mut dom, &sender, vec![watch::VaultChange::Rescan]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("liveness-degraded"), "{html}");
        assert!(html.contains("indexing the vault"), "{html}");

        // the squat removed, the next batch rebuilds from disk and the
        // arrival resolves the degradation without a gesture
        // (adr/2026-08-failed-batch-escalates-to-rescan.md)
        std::fs::remove_dir(vault.path().join(".index/index.db"))
            .expect("the squat is removed");
        feed_batch(&mut dom, &sender, vec![watch::VaultChange::Rescan]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("liveness-watching"), "healed: {html}");
        assert!(!html.contains("indexing the vault"), "resolved: {html}");
    }

    #[test]
    fn an_app_with_no_feed_keeps_the_index_it_launched_with() {
        // every other test mounts this way; the shell must simply not watch
        let vault = temp_vault();
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        assert!(!dioxus_ssr::render(&dom).contains("indexing the vault"));
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
        dom.insert_any_root_context(Box::new(VaultFeed {
            changes: Arc::new(Mutex::new(None)),
            trouble: None,
        }));
        with_reactor(|| dom.rebuild_to_vec());
        assert!(
            dioxus_ssr::render(&dom).contains("rail-id"),
            "it still renders"
        );
    }

    #[test]
    fn a_drain_already_taken_starts_no_second_task() {
        // the outcomes cell arrives empty, as it would on a second shell —
        // the `a_feed_already_taken` twin for the compute tier
        let vault = temp_vault();
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(Some(
            vault.path().to_path_buf(),
        ))));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(ComputeFeed {
            submit: Arc::new(|_| {}),
            outcomes: Arc::new(Mutex::new(None)),
            inline: false,
        }));
        with_reactor(|| dom.rebuild_to_vec());
        assert!(
            dioxus_ssr::render(&dom).contains("selected missing"),
            "it still renders"
        );
    }

    // -- the compute tier: the queued adapter, played by hand ----------------

    /// The compute tier a test can hold: submitted jobs pile up unrun, and
    /// the test decides what lands — by running a job for real or by
    /// fabricating an outcome outright (adr/2026-08-compute-tier-worker-seam.md).
    struct HeldCompute {
        jobs: Arc<Mutex<Vec<Job>>>,
        outcomes: tokio::sync::mpsc::UnboundedSender<Outcome>,
    }

    impl HeldCompute {
        /// Every job submitted since the last take, in submit order.
        fn take(&self) -> Vec<Job> {
            std::mem::take(
                &mut *self.jobs.lock().expect("the job list is healthy"),
            )
        }

        /// Lands one outcome on the shell's drain — settle to see it.
        fn land(&self, outcome: Outcome) {
            self.outcomes
                .send(outcome)
                .expect("the shell holds the drain");
        }

        /// Plays the worker: runs every queued job, lands the results and
        /// settles the drain.
        fn work(&self, dom: &mut VirtualDom) {
            for job in self.take() {
                self.land(compute::run(job));
            }
            block_on(settle(dom));
        }
    }

    /// The app under the queued adapter, with a watcher channel beside it:
    /// the threaded launch, minus the threads.
    fn scripted_app(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        HeldCompute,
        tokio::sync::mpsc::UnboundedSender<Vec<watch::VaultChange>>,
    ) {
        let (dom, clicks, _, held, sender) = scripted_app_with_keys(root);
        (dom, clicks, held, sender)
    }

    /// `scripted_app`, with the mount's keydown targets too — the last of
    /// them is the open note's own sink.
    fn scripted_app_with_keys(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        Vec<ElementId>,
        HeldCompute,
        tokio::sync::mpsc::UnboundedSender<Vec<watch::VaultChange>>,
    ) {
        set_event_converter(Box::new(TestEvents));
        let jobs = Arc::new(Mutex::new(Vec::new()));
        let queued = jobs.clone();
        let (outcome_sender, outcome_receiver) =
            tokio::sync::mpsc::unbounded_channel();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        dom.insert_any_root_context(Box::new(VaultFeed {
            changes: Arc::new(Mutex::new(Some(receiver))),
            trouble: None,
        }));
        dom.insert_any_root_context(Box::new(ComputeFeed {
            submit: Arc::new(move |job| {
                queued.lock().expect("the job list is healthy").push(job);
            }),
            outcomes: Arc::new(Mutex::new(Some(outcome_receiver))),
            inline: false,
        }));
        let mutations = with_reactor(|| dom.rebuild_to_vec());
        let clicks = listeners(&mutations, "click");
        let keys = listeners(&mutations, "keydown");
        (
            dom,
            clicks,
            keys,
            HeldCompute {
                jobs,
                outcomes: outcome_sender,
            },
            sender,
        )
    }

    #[test]
    fn a_queued_launch_mounts_empty_then_the_survey_lands() {
        let vault = temp_vault();
        let (mut dom, _, held, _sender) =
            scripted_app(Some(vault.path().to_path_buf()));
        let empty = dioxus_ssr::render(&dom);
        // the rail knows only the selection (a `missing` row until the
        // survey says otherwise), never the other days
        assert!(
            empty.contains("selected missing"),
            "the rail waits: {empty}"
        );
        assert!(!empty.contains("2026-07-21"), "{empty}");
        assert!(!empty.contains("vault-error"), "{empty}");

        // the boot queued the survey first, then the open note's fragments
        let jobs = held.take();
        assert!(
            matches!(
                jobs.first(),
                Some(Job::Survey {
                    escalated: false,
                    ..
                })
            ),
            "the launch survey is queued"
        );
        assert!(
            jobs[1..].iter().all(|job| matches!(job, Job::Fragment(_))),
            "the open note's compiles ride behind it"
        );
        let survey = jobs
            .into_iter()
            .next()
            .expect("the launch queue starts with the survey");
        held.land(compute::run(survey));
        block_on(settle(&mut dom));
        let surveyed = dioxus_ssr::render(&dom);
        assert!(
            surveyed.contains(r#"<span class="rail-id">2026-07-23</span>"#),
            "the survey filled the rail: {surveyed}"
        );
        // the fragments are still out: their blocks hold as dimmed source,
        // and the repaint queued nothing twice
        assert!(surveyed.contains("block-pending"), "{surveyed}");
        assert!(held.take().is_empty(), "the in-flight set dedups");
    }

    #[test]
    fn an_open_note_holds_dimmed_source_until_its_fragments_land() {
        let vault = temp_vault();
        let (mut dom, clicks, held, _sender) =
            scripted_app(Some(vault.path().to_path_buf()));
        let before = dioxus_ssr::render(&dom);
        assert!(before.contains("block-pending"), "{before}");
        assert!(before.contains("pending-source"), "{before}");
        assert!(!before.contains(RENDERED_NOTE), "{before}");

        // a pending region activates like any other: the region's own
        // adjacent block opens as source and the whole rest of the note
        // goes pending in its place (registration runs the grid first,
        // then the one pending region above the active trailing line
        // (adr/2026-08-cursor-split-rendering.md) — the two crumb jumps
        // and the rail's selected row)
        click(&mut dom, clicks[clicks.len() - 4]);
        assert!(
            source_of(&dom).contains("2026-07-22"),
            "the link line is the active block now: {}",
            source_of(&dom)
        );

        held.work(&mut dom);
        let after = dioxus_ssr::render(&dom);
        assert!(after.contains(RENDERED_NOTE), "the SVGs landed: {after}");
        assert!(!after.contains("block-pending"), "{after}");

        // dropping the tier closes the drain: the task ends instead of
        // waiting on a dead channel
        drop(held);
        block_on(settle(&mut dom));
    }

    /// A click landing on the pending block itself, not one of the note's
    /// CSS-drawn blocks beside it, activates that block by its own start —
    /// the same click wiring `Pane::Fragment` and `Pane::Css` carry, proven
    /// here for `Pane::Pending` directly rather than only exercised through
    /// a neighbour (adr/2026-08-css-draws-the-markup.md).
    #[test]
    fn clicking_a_pending_block_activates_it_by_its_own_start() {
        let vault = temp_vault();
        let (mut dom, clicks, held, _sender) =
            scripted_app(Some(vault.path().to_path_buf()));
        let before = dioxus_ssr::render(&dom);
        assert!(before.contains("block-pending"), "{before}");

        // the preamble (import/show/meta) is the note's only Typst
        // fallback block, so it is the one pending pane on the page
        click(&mut dom, clicks[clicks.len() - 7]);
        assert!(
            source_of(&dom).contains("#import"),
            "the pending block is the active one now: {}",
            source_of(&dom)
        );

        drop(held);
        block_on(settle(&mut dom));
    }

    #[test]
    fn a_changed_fallback_block_keeps_its_last_image_until_the_fresh_one_lands()
     {
        let vault = temp_vault();
        let (mut dom, _, keys, held, _sender) =
            scripted_app_with_keys(Some(vault.path().to_path_buf()));
        held.work(&mut dom);
        let ready = dioxus_ssr::render(&dom);
        assert!(ready.contains(RENDERED_NOTE), "{ready}");
        assert!(!ready.contains("block-stale"), "{ready}");

        // the preamble is the note's one fallback block: gg lands on its
        // first line from the trailing line the note opens on (a pure
        // grammar motion, no geometry seam to await); add a character and
        // put the block away — its content changed, so its key did
        let sink = keys[keys.len() - 1];
        press(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        // the woken block mounts its own sink: its keydown target is in
        // the motion's mutations
        let woken = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("g".into()),
            Modifiers::empty(),
        );
        let sink = listeners(&woken, "keydown")[0];
        assert!(source_of(&dom).contains("#import"), "{}", source_of(&dom));
        for key in [Key::Character("A".into()), Key::Character(" ".into())] {
            press(&mut dom, sink, key, Modifiers::empty());
        }
        // back to normal, then G wakes the last line: the preamble renders
        // again as a fallback block whose content is new
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        press(
            &mut dom,
            sink,
            Key::Character("G".into()),
            Modifiers::empty(),
        );
        let stale = dioxus_ssr::render(&dom);
        assert!(
            stale.contains("block-stale"),
            "the last image stands in: {stale}"
        );
        assert!(stale.contains("block-pending"), "{stale}");
        assert!(
            stale.contains(RENDERED_NOTE),
            "an image, not source: {stale}"
        );
        assert!(!stale.contains("pending-source"), "{stale}");

        // the fresh compile lands and the stale one leaves with it
        held.work(&mut dom);
        let fresh = dioxus_ssr::render(&dom);
        assert!(!fresh.contains("block-stale"), "{fresh}");
        assert!(!fresh.contains("block-pending"), "{fresh}");
        assert!(fresh.contains(RENDERED_NOTE), "{fresh}");

        drop(held);
        block_on(settle(&mut dom));
    }

    #[test]
    fn a_fragment_that_fails_off_thread_reports_in_its_block() {
        let vault = temp_vault();
        let (mut dom, _, held, _sender) =
            scripted_app(Some(vault.path().to_path_buf()));
        for job in held.take() {
            match job {
                // the first fragment lands broken, the rest for real
                Job::Fragment(fragment) => {
                    held.land(Outcome::Fragment {
                        key: fragment.key,
                        epoch: fragment.epoch,
                        result: Err("le typo".to_string()),
                    });
                }
                job => held.land(compute::run(job)),
            }
        }
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(html.contains("le typo"), "{html}");
    }

    #[test]
    fn a_template_touch_recompiles_the_open_notes_blocks() {
        let vault = temp_vault();
        let (mut dom, _, held, sender) =
            scripted_app(Some(vault.path().to_path_buf()));
        held.work(&mut dom);
        let ready = dioxus_ssr::render(&dom);
        assert!(!ready.contains("pending-source"), "{ready}");

        // the template changed: every cached pixel compiled against the
        // old one (adr/2026-08-template-touch-clears-caches.md)
        feed_batch(&mut dom, &sender, vec![watch::VaultChange::Template]);
        for job in held.take() {
            held.land(compute::run(job));
        }
        block_on(settle(&mut dom));
        let pending = dioxus_ssr::render(&dom);
        assert!(
            pending.contains("pending-source"),
            "the cleared blocks show their source again: {pending}"
        );

        held.work(&mut dom);
        let again = dioxus_ssr::render(&dom);
        assert!(!again.contains("pending-source"), "{again}");
    }

    #[test]
    fn a_failed_survey_degrades_escalates_once_and_heals() {
        let vault = temp_vault();
        let (mut dom, _, held, _sender) =
            scripted_app(Some(vault.path().to_path_buf()));
        held.take();
        held.land(Outcome::Survey {
            result: Err("indexing the vault: boom".to_string()),
            escalated: false,
        });
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("indexing the vault: boom"), "{html}");
        assert!(html.contains("liveness-degraded"), "{html}");
        let jobs = held.take();
        let [
            Job::Survey {
                batch,
                escalated: true,
                ..
            },
        ] = jobs.as_slice()
        else {
            panic!("the drain escalates exactly one rescan");
        };
        assert_eq!(batch.as_slice(), &[watch::VaultChange::Rescan]);

        // a rescan that itself fails escalates no further
        held.land(Outcome::Survey {
            result: Err("indexing the vault: encore".to_string()),
            escalated: true,
        });
        block_on(settle(&mut dom));
        assert!(held.take().is_empty(), "no escalation loop");
        assert!(dioxus_ssr::render(&dom).contains("liveness-degraded"));

        // a later good survey resolves the degradation without a gesture
        held.land(compute::run(compute::rescan(
            vault.path(),
            false,
            test_today(),
        )));
        block_on(settle(&mut dom));
        let healed = dioxus_ssr::render(&dom);
        assert!(healed.contains("liveness-watching"), "{healed}");
        assert!(!healed.contains("indexing the vault"), "{healed}");
    }

    #[test]
    fn a_zoomed_body_lands_late_and_stays_stale_while_recompiling() {
        let vault = temp_vault();
        let (mut dom, clicks, held, sender) =
            scripted_app(Some(vault.path().to_path_buf()));
        held.work(&mut dom);

        let (pane, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        centre_alpha(&mut dom, pane);
        press(&mut dom, keys, ctrl_equals(), Modifiers::CONTROL);
        let pending = dioxus_ssr::render(&dom);
        assert!(pending.contains("body-pending"), "{pending}");
        assert!(!pending.contains(RENDERED_NOTE), "{pending}");

        // a repaint while the compile is out queues nothing twice: an
        // unrelated landing forces the re-render, and the probe answers
        // pending without a job
        let queued = held.take();
        held.land(Outcome::Fragment {
            key: 0,
            epoch: 0,
            result: Ok(String::new()),
        });
        block_on(settle(&mut dom));
        assert!(
            held.take().is_empty(),
            "the in-flight body queued no sibling"
        );
        for job in queued {
            held.land(compute::run(job));
        }
        block_on(settle(&mut dom));
        let compiled = dioxus_ssr::render(&dom);
        assert!(compiled.contains(RENDERED_NOTE), "{compiled}");
        assert!(!compiled.contains("body-pending"), "{compiled}");

        // alpha breaks on disk; the batch invalidates its body, and the
        // stale SVG holds the slot while the recompile is out
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
        held.work(&mut dom);
        let stale = dioxus_ssr::render(&dom);
        assert!(stale.contains(RENDERED_NOTE), "the stale holds: {stale}");
        assert!(!stale.contains("body-pending"), "{stale}");
        assert!(!stale.contains("render-error"), "{stale}");

        held.work(&mut dom);
        let after = dioxus_ssr::render(&dom);
        assert!(
            after.contains("render-error"),
            "the recompile reports: {after}"
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
            Ok("collé du navigateur".to_string()),
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
            Ok("deux fois".to_string()),
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

        // a clipboard read failure captures nothing
        let (mut dom, _, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Err("read denied".to_string()),
            Some(CAPTURED_AT),
        );
        capture_chord(&mut dom, &keys);
        assert_eq!(count(), before);
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("clipboard: read denied — no text was read"),
            "{html}"
        );

        // nor does one with no clock to stamp the note by
        let (mut dom, _, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Ok("sans horloge".to_string()),
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
            Ok("pour la capture".to_string()),
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
        // the link is its own line-block now
        // (adr/2026-08-per-line-block-segmentation.md); the caret sits
        // right at its start
        let (block, keys) = activate_link(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 0);
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
            source_of(&dom).contains(r#"[[2026-summer]][[2026-07-22]]"#),
            "spliced at the caret: {}",
            source_of(&dom)
        );
        // the caret lands past the link it just wrote: the box caret
        // wears the old link's first cluster
        assert!(
            html.contains(
                r#">]]</span><span class="caret-box" data-start="15">[</span>"#
            ),
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
            source_of(&dom).starts_with(r#"[[2026-07-21]]"#),
            "the first row went in at the caret: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn a_second_typed_bracket_summons_the_picker_and_leaves_no_empty_pair() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_link(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 0);
        let bracket = || Key::Character("[".into());
        press(
            &mut dom,
            keys,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        press(&mut dom, keys, bracket(), Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains("link-picker"),
            "one bracket only pairs: {html}"
        );
        assert!(source_of(&dom).starts_with("[][["), "{}", source_of(&dom));

        let mutations =
            press_for_mutations(&mut dom, keys, bracket(), Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("link-picker"), "the second summons: {html}");
        assert!(
            source_of(&dom).starts_with("[[2026-07-22]]"),
            "the empty pair is taken back out: {}",
            source_of(&dom)
        );
        let input = listeners(&mutations, "input")[0];
        let picker_keys = listeners(&mutations, "keydown")[0];
        mount(&mut dom, listeners(&mutations, "mounted")[0]);
        type_into(&mut dom, input, "summer");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        assert!(
            source_of(&dom).starts_with("[[2026-summer]][[2026-07-22]]"),
            "one link shape, whichever way the picker opened: {}",
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
        // the heading's own markup roles tile its text across several
        // spans (adr/2026-08-css-draws-the-markup.md), so the source is
        // read back through `source_of` rather than as one contiguous
        // literal
        assert!(
            source_of(&dom).contains("= 2026-07-23"),
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
        place_caret(&mut dom, block, &hit, 4);
        let (_, picker_keys) = open_picker(&mut dom, keys);

        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("link-picker"), "{html}");
        assert!(html.contains("block-active"), "still editing: {html}");
        // the caret still stands where Ctrl+L found it
        assert!(
            html.contains(r#"class="caret-box" data-start="4""#),
            "{html}"
        );
    }

    #[test]
    fn the_todo_chord_is_blocked_while_the_picker_is_open() {
        // an overlay owns the keyboard while it stands, the same guard
        // every other buffer/pane chord here carries (Ctrl+L, Ctrl+P,
        // Ctrl+N, Ctrl+D) — the todo toggle must not silently rewrite the
        // line behind the picker
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, keys) = activate_heading(&mut dom, &clicks);
        let (_, picker_keys) = open_picker(&mut dom, keys);
        let before = source_of(&dom);

        press(
            &mut dom,
            picker_keys,
            Key::Character("t".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert_eq!(source_of(&dom), before, "the line stayed untouched");
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

    /// The fixture's selected day links `"2026-07-22"` from its own
    /// line-block, right after the heading's
    /// (adr/2026-08-per-line-block-segmentation.md).
    const IN_LINK: usize = 3;

    #[test]
    fn ctrl_enter_opens_the_time_note_under_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_link(&mut dom, &clicks);

        // inside the `[[2026-07-22]]` the link block's own text
        place_caret(&mut dom, block, &hit, IN_LINK);
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
        let (block, _) = activate_link(&mut dom, &clicks);

        // the press asks the probe where it landed: inside the day link
        *hit.lock().expect("the hit cell never poisons") = Some((0, IN_LINK));
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
        let (block, _) = activate_link(&mut dom, &clicks);

        // the caret lands in the link, but without the modifier nothing
        // follows
        place_caret(&mut dom, block, &hit, IN_LINK);
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
             \n= anonyme\n[[2026-07-23]]\n",
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
        // the link is its own line-block now
        // (adr/2026-08-per-line-block-segmentation.md): it loses its
        // outgoing link and gains a ghost one
        let (_, sink) = activate_link(&mut dom, &clicks);
        retype(&mut dom, sink, "[[fantôme]]");
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
        // the link is its own line-block now
        // (adr/2026-08-per-line-block-segmentation.md); emptying it
        // removes the outgoing link entirely
        let (_, sink) = activate_link(&mut dom, &clicks);
        retype(&mut dom, sink, " ");
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
        assert!(html.contains("ctrl+l"), "the chords show: {html}");
        assert_eq!(
            palette_labels(&dom),
            vec![
                "edit template",
                "export pdf",
                "fold jump panel",
                "fold rail",
                "follow link",
                "go to table",
                "insert link",
                "new note",
                "notices",
                "open daily",
                "open loops",
                "open next daily",
                "open next season",
                "open next weekly",
                "open note",
                "open previous daily",
                "open previous season",
                "open previous weekly",
                "open season",
                "open weekly",
                "quit",
                "search text",
                "settings",
                "toggle theme",
            ],
            "alphabetized; the note opened editing, so the caret commands \
             stand; the screen already stood on is not offered, and no \
             sheet backs a delete; open note always stands"
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
        assert_eq!(labels.len(), 24, "{labels:?}");
        assert!(labels.contains(&"insert link".to_string()), "{labels:?}");
        assert!(labels.contains(&"follow link".to_string()), "{labels:?}");
    }

    // -- the notices overlay: the history behind a palette command -----------

    #[test]
    fn an_empty_notices_history_says_so() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "notices");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">notices<"), "{html}");
        assert!(html.contains("nothing to report"), "{html}");

        // Escape closes the overlay; a second, with nothing standing and
        // nothing to close, is inert — the ladder's bottom writes nothing
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">notices<"), "{html}");
        assert!(
            !html.contains("notice-"),
            "a clean line stays clean: {html}"
        );
    }

    #[test]
    fn the_notices_overlay_lists_the_history_and_escape_closes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // a standing warning to remember: the missing template refuses
        std::fs::remove_file(vault.path().join("templates/daily.typ"))
            .expect("remove the template");
        click(&mut dom, clicks[day_cell(24)]);
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "notices");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">notices<"), "{html}");
        assert!(
            html.contains(r#"class="loops-line""#),
            "the history lists the warning: {html}"
        );

        // Escape closes the overlay; the notice itself still stands — a
        // record is not a dismissal
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">notices<"), "{html}");
        assert!(html.contains("notice-warning"), "{html}");
    }

    #[test]
    fn the_table_escape_reaches_the_overlay_then_the_notice() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // a standing warning, then the table
        std::fs::remove_file(vault.path().join("templates/daily.typ"))
            .expect("remove the template");
        click(&mut dom, clicks[day_cell(24)]);
        press(&mut dom, keys[LOGS_KEYS], Key::Enter, Modifiers::empty());
        let (_pane, _cards, table_keys) =
            table_targets_with_keys(&mut dom, &clicks);

        // the palette summons the overlay over the table too
        let (input, palette_keys) = open_palette(&mut dom, table_keys);
        type_into(&mut dom, input, "notices");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(">notices<"));

        // the ladder: the first Escape closes the overlay, the second —
        // with no sheet to close — acknowledges the notice
        press(&mut dom, table_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">notices<"));
        press(&mut dom, table_keys, Key::Escape, Modifiers::empty());
        press(
            &mut dom,
            table_keys,
            Key::Character("2".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("notice-warning"), "acknowledged: {html}");
    }

    #[test]
    fn the_notices_overlay_closes_on_a_click() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, own_click) = open_notices_overlay(&mut dom, keys[LOGS_KEYS]);
        assert!(dioxus_ssr::render(&dom).contains(">notices<"));

        click(&mut dom, own_click);
        assert!(!dioxus_ssr::render(&dom).contains(">notices<"));
    }

    #[test]
    fn the_notices_overlays_own_escape_closes_it_after_a_click() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // a click somewhere inside first — no pane holds the focus that
        // the app-level ladder would read escape from
        let (_, own_click) = open_notices_overlay(&mut dom, keys[LOGS_KEYS]);
        click(&mut dom, own_click);
        assert!(!dioxus_ssr::render(&dom).contains(">notices<"));

        // reopen and this time drive the overlay's own onkeydown directly,
        // not the pane's app-level rung
        let (own_keys, _) = open_notices_overlay(&mut dom, keys[LOGS_KEYS]);

        // only Escape is answered here; a plain key leaves the overlay up
        press(
            &mut dom,
            own_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert!(dioxus_ssr::render(&dom).contains(">notices<"));

        press(&mut dom, own_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">notices<"));
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
    fn alt_h_and_alt_l_fold_the_panes_from_normal_mode_and_the_pane() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("rail folded"), "{html}");

        // from the sink, in normal mode: the rail folds, then the jump
        press(&mut dom, sink, Key::Character("h".into()), Modifiers::ALT);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="rail folded""#), "{html}");
        press(&mut dom, sink, Key::Character("L".into()), Modifiers::ALT);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="jump folded""#), "{html}");
        // a second press unfolds
        press(&mut dom, sink, Key::Character("h".into()), Modifiers::ALT);
        press(&mut dom, sink, Key::Character("l".into()), Modifiers::ALT);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("folded"), "{html}");

        // in insert mode the same key types: alt stays insertable
        press(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        press(&mut dom, sink, Key::Character("h".into()), Modifiers::ALT);
        assert!(!dioxus_ssr::render(&dom).contains("folded"));
        assert!(source_of(&dom).contains('h'), "{}", source_of(&dom));
        press(&mut dom, sink, Key::Escape, Modifiers::empty());

        // from the pane itself, the empty day: the arm on the logs keydown
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("h".into()),
            Modifiers::ALT,
        );
        assert!(dioxus_ssr::render(&dom).contains(r#"class="rail folded""#));
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("x".into()),
            Modifiers::ALT,
        );
        assert!(dioxus_ssr::render(&dom).contains(r#"class="rail folded""#));
    }

    #[test]
    fn the_palette_folds_the_panes_on_the_logs_only() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "fold rail");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(r#"class="rail folded""#));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "fold jump");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(r#"class="jump folded""#));

        // on the table the rows are not offered
        let (_, _, table_keys) = table_targets_with_keys(&mut dom, &clicks);
        open_palette(&mut dom, table_keys);
        let labels = palette_labels(&dom);
        assert!(
            !labels.iter().any(|label| label.starts_with("fold")),
            "{labels:?}"
        );
    }

    #[test]
    fn the_palette_exports_the_open_note_beside_itself() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "export pdf");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        let pdf = std::fs::read(vault.path().join("time/2026-07-23.pdf"))
            .expect("the pdf is beside the note");
        assert!(pdf.starts_with(b"%PDF-"), "a real pdf");
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("exported time/2026-07-23.pdf"),
            "the receipt is on the line: {html}"
        );
        assert!(html.contains("notice-info"), "{html}");
    }

    #[test]
    fn an_export_that_will_not_compile_or_save_says_so_and_writes_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // the 21st's note is `#let x = (` — a compile error
        click(&mut dom, clicks[RAIL_DAY_21]);
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "export pdf");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("export: "), "the stage is named: {html}");
        assert!(html.contains("no pdf was written"), "{html}");
        assert!(!vault.path().join("time/2026-07-21.pdf").exists());

        // a dirty note the disk refuses is not exported stale: the flush
        // comes first and its refusal is the notice
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        lock_dir(&vault.path().join("time"), true);
        let (input, palette_keys) = open_palette(&mut dom, sink);
        type_into(&mut dom, input, "export pdf");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("time"), false);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-critical"), "the save refused: {html}");
        assert!(html.contains("save: "), "{html}");
        assert!(!html.contains("exported"), "nothing was exported: {html}");
        assert!(!vault.path().join("time/2026-07-21.pdf").exists());
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
        // alphabetized, `toggle theme` is the last of the 24 visible rows
        click(&mut dom, listeners(&mutations, "click")[23]);
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
    fn the_palette_runs_insert_link_at_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        // the caret is app state: the palette cannot move it, so the link
        // lands where it stood before Ctrl+P — the start of the link's
        // own line-block (adr/2026-08-per-line-block-segmentation.md)
        let (block, keys) = activate_link(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, 0);
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
            source_of(&dom).contains(r#"[[2026-summer]][[2026-07-22]]"#),
            "spliced at the caret: {}",
            source_of(&dom)
        );
    }

    #[test]
    fn the_palette_runs_follow_link_from_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_link(&mut dom, &clicks);
        place_caret(&mut dom, block, &hit, IN_LINK);
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
    fn the_palette_opens_the_loops_list_and_escape_closes_it() {
        let vault = debt_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "loops");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("loops-list"), "{html}");

        // overlays never stack (adr/2026-08-settings-overlay.md): Ctrl+P
        // now declines while the loops list stands open, so the way back
        // is the overlay's own dismissal — its Escape rung — not a second
        // palette summon
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("loops-list"), "{html}");
    }

    #[test]
    fn the_palette_open_daily_goes_back_to_today() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">21"), "{html}");

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">23"), "{html}");
    }

    #[test]
    fn a_refused_flush_keeps_selects_own_note_open() {
        // `select`'s own flush guard (adr/2026-08-sheet-reuses-the-one-editor.md):
        // a save that cannot reach disk must not still swap the note out
        // from under the buffer that holds the unsaved text
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        retype(&mut dom, sink, "= presque perdu\n");

        lock_dir(&vault.path().join("time"), true);
        click(&mut dom, clicks[RAIL_DAY_21]);
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("time"), false);

        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "the refused flush held the selection: {html}"
        );
        assert!(html.contains("notice-critical"), "{html}");
    }

    // -- ctrl+d: the palette's daily command's chord, from both panes -------

    #[test]
    fn ctrl_d_opens_todays_daily_from_the_logs_pane() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">21"), "{html}");

        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("d".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">23"), "{html}");
    }

    #[test]
    fn ctrl_d_opens_todays_daily_from_the_table_pane() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        press(
            &mut dom,
            keys,
            Key::Character("d".into()),
            Modifiers::CONTROL,
        );

        // the chord lands the daily on the temporal (logs) screen itself
        // (todo 25) — no round trip through the chrome icon is needed to
        // see it
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(!html.contains(r#"class="table""#), "{html}");
        assert!(html.contains("cal-day has-note selected\">23"), "{html}");
    }

    #[test]
    fn ctrl_d_from_an_open_sheet_still_lands_on_the_daily_in_the_logs() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        press(
            &mut dom,
            keys,
            Key::Character("d".into()),
            Modifiers::CONTROL,
        );

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(
            !html.contains(r#"class="sheet""#),
            "the sheet closed: {html}"
        );
        assert!(html.contains("cal-day has-note selected\">23"), "{html}");
    }

    #[test]
    fn ctrl_d_does_nothing_while_the_palette_is_open() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">21"), "{html}");

        open_palette(&mut dom, keys[LOGS_KEYS]);
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("d".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">21"), "{html}");
    }

    // -- the settings overlay: theme and font size ---------------------------
    // (adr/2026-08-settings-overlay.md)

    #[test]
    fn ctrl_comma_opens_settings_from_the_logs_screen() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("command-palette settings"), "{html}");
        assert!(html.contains(">settings<"), "{html}");

        // the escape ladder's own rung — defence in depth behind the
        // overlay's own onkeydown, exercised separately below
        press(&mut dom, keys[LOGS_KEYS], Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command-palette settings"), "{html}");
    }

    #[test]
    fn ctrl_comma_opens_settings_from_the_table_screen() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_settings_overlay(&mut dom, keys);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("command-palette settings"), "{html}");

        // the table pane's own ladder rung, the logs arm's twin
        press(&mut dom, keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command-palette settings"), "{html}");
    }

    #[test]
    fn the_overlays_own_escape_closes_it_and_a_plain_key_does_not() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (settings_keys, _) =
            open_settings_overlay(&mut dom, keys[LOGS_KEYS]);

        // only Escape is answered here; a plain key leaves the overlay up
        press(
            &mut dom,
            settings_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert!(
            dioxus_ssr::render(&dom).contains("command-palette settings"),
            "{}",
            dioxus_ssr::render(&dom)
        );

        press(&mut dom, settings_keys, Key::Escape, Modifiers::empty());
        assert!(
            !dioxus_ssr::render(&dom).contains("command-palette settings"),
            "{}",
            dioxus_ssr::render(&dom)
        );
    }

    #[test]
    fn ctrl_comma_does_nothing_while_the_palette_is_open() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_palette(&mut dom, keys[LOGS_KEYS]);
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character(",".into()),
            Modifiers::CONTROL,
        );
        assert!(
            !dioxus_ssr::render(&dom).contains("command-palette settings"),
            "{}",
            dioxus_ssr::render(&dom)
        );
    }

    /// Overlays never stack (adr/2026-08-settings-overlay.md): notices and
    /// loops open from every screen now (todo 24), so Ctrl+, and Ctrl+P must
    /// decline while either is up, not only while the palette-family
    /// overlays are.
    #[test]
    fn ctrl_comma_does_nothing_while_notices_are_open() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_notices_overlay(&mut dom, keys[LOGS_KEYS]);
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character(",".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command-palette settings"), "{html}");
        assert!(html.contains(">notices<"), "notices stayed up: {html}");
    }

    #[test]
    fn ctrl_comma_does_nothing_while_loops_are_open() {
        let vault = debt_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_loops_overlay(&mut dom, clicks[EMBER]);
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character(",".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command-palette settings"), "{html}");
        assert!(
            html.contains("loops-list"),
            "the loops list stayed up: {html}"
        );
    }

    #[test]
    fn ctrl_p_does_nothing_while_notices_are_open() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_notices_overlay(&mut dom, keys[LOGS_KEYS]);
        press(&mut dom, keys[LOGS_KEYS], ctrl_p(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">commands<"), "{html}");
        assert!(html.contains(">notices<"), "notices stayed up: {html}");
    }

    #[test]
    fn ctrl_p_does_nothing_while_loops_are_open() {
        let vault = debt_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_loops_overlay(&mut dom, clicks[EMBER]);
        press(&mut dom, keys[LOGS_KEYS], ctrl_p(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">commands<"), "{html}");
        assert!(
            html.contains("loops-list"),
            "the loops list stayed up: {html}"
        );
    }

    #[test]
    fn the_settings_controls_carry_hover_tooltips() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"title="toggle theme""#), "{html}");
        assert!(html.contains(r#"title="decrease font size""#), "{html}");
        assert!(html.contains(r#"title="increase font size""#), "{html}");
    }

    #[test]
    fn the_settings_theme_button_flips_the_theme() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, clicks) = open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">dark<"), "starts on dark: {html}");

        click(&mut dom, clicks[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"data-theme="light""#), "{html}");
        assert!(html.contains(">light<"), "the button follows: {html}");
    }

    #[test]
    fn the_settings_plus_button_raises_the_font_size_and_clamps_at_28() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, clicks) = open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        // six presses from 18px: 20, 22, 24, 26, 28, then clamped at 28
        for _ in 0..6 {
            click(&mut dom, clicks[2]);
        }
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("--prose-size: 28px"), "{html}");
        assert!(html.contains(">28px<"), "{html}");
    }

    #[test]
    fn the_settings_minus_button_lowers_the_font_size_and_clamps_at_12() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, clicks) = open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        // six presses from 18px: 16, 14, 12, then clamped at 12
        for _ in 0..6 {
            click(&mut dom, clicks[1]);
        }
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("--prose-size: 12px"), "{html}");
        assert!(html.contains(">12px<"), "{html}");
    }

    #[test]
    fn the_palette_runs_settings_and_opens_the_overlay() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "settings");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("command-palette settings"), "{html}");
        assert!(
            !html.contains(">commands<"),
            "the command palette itself closed: {html}"
        );
    }

    #[test]
    fn opening_next_daily_over_a_sheet_flushes_and_closes_it() {
        // the blocker this guards against: a time-navigation command whose
        // target has to be created must not swap the editor out from under
        // an open sheet the way a raw `editor.set` would — the sheet's own
        // note must flush and the sheet must close, exactly as `select`
        // already does for every other navigation
        // (adr/2026-08-time-navigation-commands.md)
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        let opened = open_sheet_on(&mut dom, pane, cards[0]);
        let (_, sink) = sheet_block_targets(&opened);
        // typed but inside the quiet window: only a flush can save it
        retype(&mut dom, sink, "= alpha presque perdu\n");

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "open next daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        let saved =
            std::fs::read_to_string(vault.path().join("permanent/alpha.typ"))
                .expect("the note is readable");
        assert!(
            saved.contains("alpha presque perdu"),
            "the sheet's edit flushed before the swap: {saved}"
        );
        let html = dioxus_ssr::render(&dom);
        assert!(
            !html.contains(r#"class="sheet""#),
            "the sheet closed: {html}"
        );
        assert!(
            vault.path().join("time/2026-07-24.typ").exists(),
            "the daily note was created"
        );
    }

    // -- the palette's other eight time-navigation commands ------------------
    // (adr/2026-08-time-navigation-commands.md)

    #[test]
    fn previous_daily_skips_a_gap() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-10.typ"),
            time_note("2026-07-10", "daily"),
        )
        .expect("seed an earlier daily note");
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open previous daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">10"),
            "the nearest earlier daily note, skipping the notes-less \
             stretch from 11 to 20: {html}"
        );
    }

    #[test]
    fn previous_daily_with_nothing_before_it_leaves_the_selection_untouched() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("cal-day has-note selected\">21"), "{html}");

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open previous daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">21"),
            "no note precedes 21, so the selection holds: {html}"
        );
        assert!(
            !vault.path().join("time/2026-07-20.typ").exists(),
            "previous never writes"
        );
    }

    #[test]
    fn next_daily_selects_without_creating_when_the_target_already_exists() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open next daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the day after 21 is already on disk: {html}"
        );
    }

    #[test]
    fn next_daily_creates_the_file_when_missing() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_23]);

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open next daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            vault.path().join("time/2026-07-24.typ").exists(),
            "next daily creates the missing file from the template"
        );
        assert!(
            dioxus_ssr::render(&dom).contains("2026-07-24"),
            "the editor opened the freshly created note: {}",
            dioxus_ssr::render(&dom)
        );
    }

    #[test]
    fn previous_weekly_with_nothing_before_it_leaves_the_selection_untouched()
    {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open previous weekly");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            dioxus_ssr::render(&dom).contains("2026-07-23"),
            "no weekly note precedes w30, so today's daily stays open: {}",
            dioxus_ssr::render(&dom)
        );
        assert!(!vault.path().join("time/2026-w29.typ").exists());
    }

    #[test]
    fn previous_season_with_nothing_before_it_leaves_the_selection_untouched()
    {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open previous season");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            dioxus_ssr::render(&dom).contains("2026-07-23"),
            "no season note precedes summer, so today's daily stays open: {}",
            dioxus_ssr::render(&dom)
        );
        assert!(!vault.path().join("time/2026-spring.typ").exists());
    }

    #[test]
    fn next_daily_reports_a_create_failure_when_the_template_is_missing() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        std::fs::remove_file(vault.path().join("templates/daily.typ"))
            .expect("remove the template");
        click(&mut dom, clicks[RAIL_DAY_23]);

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open next daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-warning"), "{html}");
        assert!(html.contains("UnknownTemplate"), "{html}");
        assert!(!vault.path().join("time/2026-07-24.typ").exists());
        // a refused creation must not move the selection onto a period
        // with no note behind it (adr/2026-08-time-navigation-commands.md)
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "the selection held: {html}"
        );
    }

    #[test]
    fn next_weekly_lands_on_the_right_id() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open next weekly");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            vault.path().join("time/2026-w31.typ").exists(),
            "the week after w30, the Monday jiff computes"
        );
        assert!(
            dioxus_ssr::render(&dom).contains("2026-w31"),
            "the editor opened it: {}",
            dioxus_ssr::render(&dom)
        );
    }

    #[test]
    fn next_season_lands_on_the_right_id() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open next season");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            vault.path().join("time/2026-autumn.typ").exists(),
            "summer rolls into autumn"
        );
        assert!(
            dioxus_ssr::render(&dom).contains("2026-autumn"),
            "the editor opened it: {}",
            dioxus_ssr::render(&dom)
        );
    }

    #[test]
    fn the_palette_open_weekly_lands_on_todays_week() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[GUTTER_W31]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no note for w31"), "{html}");

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open weekly");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            dioxus_ssr::render(&dom).contains("2026-w30"),
            "today's own week, already on disk: {}",
            dioxus_ssr::render(&dom)
        );
        assert!(
            !vault.path().join("time/2026-w31.typ").exists(),
            "open weekly does not create"
        );
    }

    #[test]
    fn the_palette_open_season_lands_on_todays_season() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[SEASON_AUTUMN]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no note for autumn 2026"), "{html}");

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open season");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            dioxus_ssr::render(&dom).contains("2026-summer"),
            "today's own season, already on disk: {}",
            dioxus_ssr::render(&dom)
        );
        assert!(
            !vault.path().join("time/2026-autumn.typ").exists(),
            "open season does not create"
        );
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
        place_caret(&mut dom, block, &hit, 4);

        let (_, palette_keys) = open_palette(&mut dom, keys);
        press(&mut dom, palette_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("block-active"), "still editing: {html}");
        // the caret is app state the palette never touched
        assert!(
            html.contains(r#"class="caret-box" data-start="4""#),
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
        // three rows, alphabetized: open previous daily, open previous
        // season, open previous weekly
        type_into(&mut dom, input, "previous");
        let selected = |dom: &VirtualDom| {
            let html = dioxus_ssr::render(dom);
            html.split("palette-row selected")
                .nth(1)
                .and_then(|rest| rest.split(r#"palette-label">"#).nth(1))
                .and_then(|rest| rest.split('<').next())
                .map(str::to_string)
                .unwrap_or_else(|| panic!("no highlighted row: {html}"))
        };
        assert_eq!(
            selected(&dom),
            "open previous daily",
            "the first row is lit"
        );

        press(&mut dom, palette_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, palette_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, palette_keys, Key::ArrowDown, Modifiers::empty());
        assert_eq!(
            selected(&dom),
            "open previous weekly",
            "the last row holds"
        );

        press(&mut dom, palette_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, palette_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, palette_keys, Key::ArrowUp, Modifiers::empty());
        assert_eq!(
            selected(&dom),
            "open previous daily",
            "the first one holds"
        );
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

        // an unbound ctrl chord still bubbles out of the palette (it lives
        // beside the panes, not inside one) instead of being swallowed;
        // nothing claims it, so nothing changes and the palette stays up
        press(
            &mut dom,
            palette_keys,
            Key::Character("z".into()),
            Modifiers::CONTROL,
        );
        assert_eq!(
            palette_labels(&dom),
            vec!["toggle theme"],
            "the bubbled chord matched nothing"
        );

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
        press(&mut dom, keys, Key::Escape, Modifiers::SHIFT);
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

    // -- Ctrl+O: the one note switcher
    //    (adr/2026-09-ctrl-o-is-the-one-note-switcher.md) -------------------

    /// An empty visit log is not a no-op the way the picker this
    /// replaces made it: the
    /// switcher opens on nothing so the query can be typed, and says
    /// which emptiness it is.
    #[test]
    fn the_switcher_opens_on_an_empty_visit_log() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));

        let (input, _picker_keys, _) =
            open_switcher(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">open note<"), "the switcher opened: {html}");
        assert_eq!(picker_ids(&dom), Vec::<String>::new());
        assert!(html.contains("no note visited yet"), "{html}");

        // and a query reaches the whole vault from that same empty list —
        // the day showing is left out, every other note is offered
        type_into(&mut dom, input, "2026");
        assert_eq!(
            picker_ids(&dom),
            ["2026-07-21", "2026-07-22", "2026-summer", "2026-w30"],
            "the current selection is not among its own destinations"
        );
    }

    /// The typed half is the whole index, not the table's cards: a time
    /// note and a capture are both switchable, which the jump overlay's
    /// card restriction refused.
    #[test]
    fn a_typed_query_matches_every_note_by_id_and_title() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);

        let (input, picker_keys, _) = open_switcher(&mut dom, keys[LOGS_KEYS]);
        // the empty query is the log alone
        assert_eq!(picker_ids(&dom), ["2026-07-23"]);

        type_into(&mut dom, input, "a");
        assert_eq!(
            picker_ids(&dom),
            ["alpha", "capture-idea"],
            "a permanent note and a capture, both by id"
        );

        // and the time notes the jump overlay could never offer, the day
        // showing still left out of its own list
        type_into(&mut dom, input, "2026-0");
        assert_eq!(picker_ids(&dom), ["2026-07-22", "2026-07-23"]);

        // a query nothing matches leaves the other message, and enter
        // over it does nothing
        type_into(&mut dom, input, "xyzzy");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching note"), "{html}");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">open note<"), "still open: {html}");
        assert!(
            html.contains("cal-day has-note selected\">21"),
            "nothing landed: {html}"
        );
    }

    /// The pure row rule under the two halves, without a VirtualDom.
    #[test]
    fn switcher_rows_read_the_log_empty_and_the_index_typed() {
        let entries: Vec<links::Completion> = ["alpha", "beta"]
            .into_iter()
            .map(|id| links::Completion {
                id: id.to_string(),
                title: None,
            })
            .collect();
        let frozen = Switcher {
            recent: vec![links::Completion {
                id: "beta".to_string(),
                title: Some("Beta".to_string()),
            }],
            entries,
        };
        assert_eq!(
            switcher_rows(&frozen, "")
                .into_iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            ["beta"],
            "no query: where you have been"
        );
        assert_eq!(
            switcher_rows(&frozen, "a")
                .into_iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            ["alpha", "beta"],
            "a query: everywhere you could go"
        );
    }

    /// The log read as notes: newest first, one row per note whichever
    /// surface it was visited on, the note showing left out, and the
    /// index's title carried when it has one.
    #[test]
    fn recent_notes_folds_the_log_by_note_and_drops_the_current_one() {
        let entries = vec![links::Completion {
            id: "alpha".to_string(),
            title: Some("Alpha".to_string()),
        }];
        let history = vec![
            Visit::Logs((NoteType::Daily, "2026-07-21".to_string())),
            Visit::Sheet("alpha".to_string()),
            Visit::Logs((NoteType::Daily, "2026-07-21".to_string())),
            Visit::Sheet("beta".to_string()),
        ];

        assert_eq!(
            recent_notes(&history, "beta", &entries),
            vec![
                links::Completion {
                    id: "2026-07-21".to_string(),
                    title: None,
                },
                links::Completion {
                    id: "alpha".to_string(),
                    title: Some("Alpha".to_string()),
                },
            ],
            "newest first, deduplicated by note, the current one dropped"
        );

        // the same note reached through both surfaces is one row, and
        // excluding by id drops it whichever surface shows it now
        let both = vec![
            Visit::Sheet("alpha".to_string()),
            Visit::Logs((NoteType::Daily, "alpha".to_string())),
        ];
        assert!(recent_notes(&both, "alpha", &entries).is_empty());
    }

    #[test]
    fn ctrl_o_opens_the_switcher_and_enter_lands_on_the_visit() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        assert!(
            dioxus_ssr::render(&dom)
                .contains("cal-day has-note selected\">23")
        );

        click(&mut dom, clicks[RAIL_DAY_21]);
        let (_input, picker_keys, _) =
            open_switcher(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">open note<"), "the switcher opened: {html}");
        assert_eq!(picker_ids(&dom), ["2026-07-23"]);

        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">open note<"), "the switcher closed: {html}");
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "enter landed on the visit: {html}"
        );

        // the landing was a real visit: the note just left is now the
        // log's newest entry, so the picker can bounce
        let (_input, picker_keys, _) =
            open_switcher(&mut dom, keys[LOGS_KEYS]);
        assert_eq!(picker_ids(&dom), ["2026-07-21"]);
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        assert!(
            dioxus_ssr::render(&dom)
                .contains("cal-day has-note selected\">21"),
            "the bounce landed back"
        );
    }

    #[test]
    fn the_switcher_lists_distinct_visits_newest_first_and_skips_the_current()
    {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // 23 -> 22 -> 23 -> 21 -> 22: the log holds [23, 22, 23, 21], so
        // newest-first dedup reads 21, 23 — the repeated 23 folds into its
        // newest occurrence and the current selection (22) is left out
        click(&mut dom, clicks[RAIL_DAY_22]);
        click(&mut dom, clicks[RAIL_DAY_23]);
        click(&mut dom, clicks[RAIL_DAY_21]);
        click(&mut dom, clicks[RAIL_DAY_22]);

        let (_input, picker_keys, _) =
            open_switcher(&mut dom, keys[LOGS_KEYS]);
        assert_eq!(picker_ids(&dom), ["2026-07-21", "2026-07-23"]);

        // the arrows move the highlight; enter takes the second row
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "enter landed on the highlighted row: {html}"
        );
    }

    #[test]
    fn the_switcher_query_leaves_the_log_for_the_whole_vault() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        click(&mut dom, clicks[RAIL_DAY_22]);

        let (input, picker_keys, _) = open_switcher(&mut dom, keys[LOGS_KEYS]);
        assert_eq!(picker_ids(&dom), ["2026-07-21", "2026-07-23"]);

        // the arrows clamp at both ends of the list
        press(&mut dom, picker_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());

        // a query leaves the log behind and matches the whole index —
        // 2026-07-22 is the day showing and stays out of its own list
        type_into(&mut dom, input, "2026-07-2");
        assert_eq!(picker_ids(&dom), ["2026-07-21", "2026-07-23"]);

        // an overlay is up: the summoning chords refuse to stack another
        press(&mut dom, picker_keys, ctrl_p(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains("command…"), "{html}");

        // a query nothing matches leaves a message, and enter does nothing
        type_into(&mut dom, input, "xyzzy");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching note"), "{html}");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("no matching note"), "{html}");
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "nothing landed: {html}"
        );

        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">open note<"), "escape closed it: {html}");
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the selection never moved: {html}"
        );
    }

    /// The flush discipline: the buffer being left reaches disk before
    /// the note is replaced, and a refusal leaves the switcher up over
    /// the list it was showing rather than closed over a note that never
    /// moved (adr/2026-09-ctrl-o-is-the-one-note-switcher.md). Both
    /// branches refuse the same way — `show_sheet`'s flush and
    /// `select`'s.
    #[test]
    fn a_refused_flush_keeps_the_switcher_open() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        // the logs hold today's note and its directory refuses every
        // write, so the flush the landing runs cannot land
        lock_dir(&vault.path().join("time"), true);

        // the sheet branch: alpha has a card, so the landing is
        // `show_sheet`'s, and its flush guard returns before the sheet
        let (input, picker_keys, _) = open_switcher(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "alpha");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">open note<"), "the switcher stands: {html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");

        // the time branch: the same refusal one seam over, in `select`
        type_into(&mut dom, input, "2026-07-21");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        lock_dir(&vault.path().join("time"), false);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">open note<"), "the switcher stands: {html}");
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "the selection never moved: {html}"
        );
    }

    #[test]
    fn a_logs_visit_clicked_from_a_sheet_lands_and_closes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);

        // opening the sheet is itself a visit: what stood on the logs a
        // moment ago is in the list, newest first
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        let (_input, _picker_keys, rows) = open_switcher(&mut dom, keys);
        assert_eq!(picker_ids(&dom), ["2026-07-21", "2026-07-23"]);

        // a click on a row lands the same way enter does
        click(&mut dom, rows[0]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "back to the logs: {html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(
            html.contains("cal-day has-note selected\">21"),
            "back to the prior selection: {html}"
        );
    }

    #[test]
    fn the_palette_runs_open_note() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);

        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open note");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">open note<"), "the switcher opened: {html}");
        assert_eq!(picker_ids(&dom), ["2026-07-23"]);
    }

    #[test]
    fn a_sheet_visit_reopens_from_the_switcher() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        // the palette's own "open daily" reaches `select` directly, with
        // the sheet still open — the sheet itself is what gets pushed
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "open daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(
            !dioxus_ssr::render(&dom).contains(r#"class="sheet""#),
            "the sheet closed under the direct select"
        );

        let (_input, picker_keys, _) = open_switcher(&mut dom, keys);
        assert_eq!(picker_ids(&dom), ["alpha"]);
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains(r#"class="sheet""#),
            "the sheet visit reopened: {html}"
        );
    }

    #[test]
    fn ctrl_d_from_the_table_records_the_sheet_it_closed() {
        // regression (final review): `go_logs` led and closed the sheet
        // before `select` pushed, so the log recorded the logs selection
        // the sheet stood over instead of the sheet itself
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        let landed = press_for_mutations(
            &mut dom,
            keys,
            Key::Character("d".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"class="logs""#), "{html}");
        assert!(
            html.contains("cal-day has-note selected\">23"),
            "the daily opened: {html}"
        );

        // the remounted logs pane's own keydown is in the landing's
        // mutations — the pane registers before the sink
        let logs_keys = listeners(&landed, "keydown")[0];
        let (_input, _picker_keys, _) = open_switcher(&mut dom, logs_keys);
        assert_eq!(
            picker_ids(&dom),
            ["alpha"],
            "the log recorded the sheet the chord closed"
        );
    }

    #[test]
    fn push_visit_drops_the_oldest_once_the_cap_is_reached() {
        let mut history: Vec<Visit> = (0..HISTORY_CAP)
            .map(|n| Visit::Logs((NoteType::Daily, format!("day-{n}"))))
            .collect();

        push_visit(
            &mut history,
            Visit::Logs((NoteType::Daily, "overflow".to_string())),
        );

        assert_eq!(history.len(), HISTORY_CAP, "the stack stays bounded");
        assert_eq!(
            history.first(),
            Some(&Visit::Logs((NoteType::Daily, "day-1".to_string()))),
            "the oldest entry is the one dropped"
        );
        assert_eq!(
            history.last(),
            Some(&Visit::Logs((NoteType::Daily, "overflow".to_string())))
        );
    }

    /// From the table the switcher opens the note rather than panning to
    /// its card, and a time note lands on the logs the way a loop line
    /// does — the destination the jump overlay had no way to reach
    /// (adr/2026-09-ctrl-o-is-the-one-note-switcher.md).
    #[test]
    fn the_switcher_opens_from_the_table_and_lands_by_category() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (input, picker_keys, _) = open_switcher(&mut dom, keys);
        assert!(dioxus_ssr::render(&dom).contains(">open note<"));
        type_into(&mut dom, input, "alpha");
        assert_eq!(picker_ids(&dom), ["alpha"]);
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">open note<"), "the switcher closed: {html}");
        assert!(
            html.contains(r#"class="sheet""#),
            "the sheet opened: {html}"
        );

        // and a time note from the same sheet lands on the logs, the loops
        // list's own category rule
        let (input, picker_keys, _) = open_switcher(&mut dom, keys);
        type_into(&mut dom, input, "2026-07-21");
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">open note<"), "{html}");
        assert!(html.contains(r#"class="logs""#), "on the logs: {html}");
        assert!(!html.contains(r#"class="sheet""#), "{html}");
        assert!(
            html.contains("cal-day has-note selected\">21"),
            "the day is selected: {html}"
        );
    }

    /// The switcher absorbs the keys every overlay absorbs, and refuses
    /// to stack a second copy of itself. (A row's own click lands in
    /// `a_logs_visit_clicked_from_a_sheet_lands_and_closes_it`.)
    #[test]
    fn the_switcher_absorbs_stray_keys_and_never_stacks() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (_input, picker_keys, _) = open_switcher(&mut dom, keys);
        press(&mut dom, picker_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, picker_keys, Key::ArrowUp, Modifiers::empty());
        press(
            &mut dom,
            picker_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        press(&mut dom, keys, ctrl_o(), Modifiers::CONTROL);
        press(&mut dom, picker_keys, ctrl_o(), Modifiers::CONTROL);
        assert!(dioxus_ssr::render(&dom).contains(">open note<"));

        press(&mut dom, picker_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">open note<"));
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
        let (_, switcher_keys) = open_overlay(&mut dom, table_keys, ctrl_o());
        press(&mut dom, switcher_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">open note<"));
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

        // the database gone entirely: the filter declines to open at all
        replace_database_with_a_directory(vault.path());
        press(&mut dom, keys, ctrl_f(), Modifiers::CONTROL);
        assert!(!dioxus_ssr::render(&dom).contains(">filter<"));

        // the switcher still opens — its recent half is app state and
        // needs no read — and only its typed half is empty; the notice
        // itself is asserted where a notice line is drawn, in
        // `the_switcher_resolves_a_standing_index_notice`
        // (adr/2026-09-ctrl-o-is-the-one-note-switcher.md)
        press(&mut dom, keys, ctrl_o(), Modifiers::CONTROL);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">open note<"), "{html}");
        assert_eq!(picker_ids(&dom), Vec::<String>::new());
    }

    /// The rows are frozen at open, so one can name a note the vault has
    /// since lost: the landing reports through the sheet's own lookup
    /// rather than going silent, and the switcher closes over it.
    #[test]
    fn a_row_whose_note_left_meanwhile_reports_and_closes() {
        let vault = temp_vault();
        let (mut dom, clicks, sender) =
            watched_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        let (input, picker_keys, _) = open_switcher(&mut dom, keys);
        type_into(&mut dom, input, "alpha");

        // the note leaves while the overlay holds its frozen entries
        std::fs::remove_file(vault.path().join("permanent/alpha.typ"))
            .expect("the note is deleted");
        feed_batch(
            &mut dom,
            &sender,
            vec![watch::VaultChange::Removed(PathBuf::from(
                "permanent/alpha.typ",
            ))],
        );
        press(&mut dom, picker_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">open note<"), "the switcher closed: {html}");
        assert!(
            html.contains("notice-warning"),
            "the failed lookup reports: {html}"
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

        // and the switcher the same way
        let (_, switcher_keys) = open_overlay(&mut dom, block_keys, ctrl_o());
        press(&mut dom, switcher_keys, Key::Escape, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains("block-active"));
        press(
            &mut dom,
            block_keys,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
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

        // the screen round trip clears it; then the switcher runs the
        // same way, from the table as from the logs
        click(&mut dom, clicks[CHROME_LOGS]);
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "open note");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(">open note<"));
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

    /// A bare key that reaches the sink while an overlay is up was typed
    /// at the overlay before its focus grab landed: it joins the query and
    /// is never a motion (adr/2026-09-overlay-keys-relay-before-focus-lands.md)
    #[test]
    fn a_key_at_the_sink_under_the_creator_joins_its_query_not_the_grammar() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_link(&mut dom, &clicks);
        let before = source_of(&dom);
        open_creator(&mut dom, keys[LOGS_KEYS]);

        // "c" then "x": an operator and a delete in normal mode, letters
        // in the overlay; Backspace takes the "x" back and Enter, with no
        // accept path to relay into, is dropped rather than read as a
        // motion — the ADR's known ceiling
        for key in [
            Key::Character("c".into()),
            Key::Character("x".into()),
            Key::Backspace,
            Key::Enter,
        ] {
            press(&mut dom, sink, key, Modifiers::empty());
        }
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"value="c""#), "{html}");
        assert!(html.contains(">new note<"), "still step one: {html}");
        assert_eq!(
            picker_ids(&dom),
            vec!["source", "concept", "claim", "project"]
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before, "the note behind never moved");
    }

    /// The relay's letters reach the field by a patch, and a key typed at
    /// the field before that patch lands is reported on a stale field: as
    /// itself alone, or on top of a type name the step change already
    /// cleared. Either way the report is a delta, never the query
    /// (adr/2026-09-an-input-event-is-a-delta-against-what-the-field-showed.md)
    #[test]
    fn a_key_the_field_reports_on_a_stale_value_is_read_as_a_delta() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_link(&mut dom, &clicks);
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        for letter in ["c", "o", "n", "c"] {
            press(
                &mut dom,
                sink,
                Key::Character(letter.into()),
                Modifiers::empty(),
            );
        }
        // the field never saw the four letters: it reports the fifth alone
        type_into(&mut dom, input, "e");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"value="conce""#), "{html}");
        // two patches landed before the next key, all of them before the last
        type_into(&mut dom, input, "cop");
        type_into(&mut dom, input, "concept");
        assert_eq!(picker_ids(&dom), vec!["concept"]);

        // the step change clears the query while the field still shows the
        // type name under the title's first letter
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        type_into(&mut dom, input, "conceptt");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"value="t""#), "{html}");
        type_into(&mut dom, input, "titled before focus");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        assert!(
            vault
                .path()
                .join("permanent/titled-before-focus.typ")
                .exists(),
            "the title is what was typed after the step change"
        );
    }

    #[test]
    fn a_key_at_the_pane_under_the_palette_joins_its_query() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        open_palette(&mut dom, keys[LOGS_KEYS]);
        press(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("n".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"value="n""#), "{html}");
        let labels = palette_labels(&dom);
        assert!(!labels.is_empty());
        assert!(labels.iter().all(|label| label.contains('n')), "{labels:?}");
    }

    #[test]
    fn a_key_at_the_table_pane_under_the_creator_joins_its_query() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let mutations = press_for_mutations(
            &mut dom,
            keys[LOGS_KEYS],
            Key::Character("1".into()),
            Modifiers::CONTROL,
        );
        let table_keys = listeners(&mutations, "keydown")[0];
        open_creator(&mut dom, table_keys);
        press(
            &mut dom,
            table_keys,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        assert!(dioxus_ssr::render(&dom).contains(r#"value="i""#));
        assert_eq!(picker_ids(&dom), vec!["organisation", "claim", "idea"]);
    }

    /// The three overlays with no query drop a bare key rather than let
    /// the grammar run it; with nothing up the same key is the delete it
    /// always was.
    #[test]
    fn a_key_at_the_sink_under_a_list_overlay_is_dropped_not_run() {
        // one typeless note is the debt the loops overlay needs to render;
        // with debt the ember takes click listener 2 (`EMBER`), so every
        // block index of the fixture day note sits one higher than in a
        // clean vault
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/typeless.typ"),
            "#import \"/templates/template.typ\": *\n#show: note\n\
             #meta(id: \"typeless\", created: \"2026-07-01\")\n\n= typeless\n",
        )
        .expect("the fixture vault is writable");
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_block(&mut dom, clicks[BLOCK_LINK + 1]);
        let before = source_of(&dom);
        assert!(before.contains("[["), "the link line is active: {before}");
        // dd, not x: the click leaves the caret past the line's end, where
        // x has nothing under it; dd cuts the line from anywhere. Two keys
        // also prove a leaked first d never completes into a cut
        let dd = |dom: &mut VirtualDom| {
            for _ in 0..2 {
                press(
                    dom,
                    sink,
                    Key::Character("d".into()),
                    Modifiers::empty(),
                );
            }
        };
        let gone = |dom: &VirtualDom, overlay: &str| {
            let html = dioxus_ssr::render(dom);
            assert!(!html.contains(overlay), "{overlay} closed: {html}");
        };

        let (loops_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);
        dd(&mut dom);
        press(&mut dom, loops_keys, Key::Escape, Modifiers::empty());
        gone(&dom, "loops-list");
        let (settings_keys, _) =
            open_settings_overlay(&mut dom, keys[LOGS_KEYS]);
        dd(&mut dom);
        press(&mut dom, settings_keys, Key::Escape, Modifiers::empty());
        gone(&dom, "command-palette settings");
        let (notices_keys, _) =
            open_notices_overlay(&mut dom, keys[LOGS_KEYS]);
        dd(&mut dom);
        press(&mut dom, notices_keys, Key::Escape, Modifiers::empty());
        gone(&dom, ">notices<");
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before, "six keys, nothing moved");

        dd(&mut dom);
        block_on(settle(&mut dom));
        assert_ne!(source_of(&dom), before, "with nothing up, dd cuts");
    }

    #[test]
    fn ctrl_n_lists_the_nine_types_and_typing_narrows() {
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

    // -- the app indexes its own writes ---------------------------------
    // (adr/2026-09-the-app-indexes-its-own-writes.md): `rendered_app` and
    // `capture_app` inject no `VaultFeed`, so no watcher runs and no batch
    // is ever fed in here — the only way the index below can hold the new
    // note is the app's own `compute::touched` submit at the write seam.
    // These fail against the pre-fix binary, which left the index waiting
    // on a watcher that was never even started.

    #[test]
    fn vault_relative_strips_the_vault_root() {
        assert_eq!(
            vault_relative(
                Path::new("/vault"),
                Path::new("/vault/permanent/a.typ")
            ),
            PathBuf::from("permanent/a.typ")
        );
    }

    #[test]
    fn vault_relative_falls_back_to_the_whole_path_outside_the_root() {
        // defensive only: every real caller passes a path `create` or
        // `template` wrote under `root`, so this branch never fires in
        // production — it exists so the fallback is `unwrap_or`, not a
        // naked unwrap
        assert_eq!(
            vault_relative(Path::new("/vault"), Path::new("/elsewhere/a.typ")),
            PathBuf::from("/elsewhere/a.typ")
        );
    }

    #[test]
    fn dir_category_reads_the_leading_directory() {
        assert_eq!(
            dir_category(Path::new("time/2026-07-20.typ")),
            NoteCategory::Time
        );
        assert_eq!(
            dir_category(Path::new("capture/x.typ")),
            NoteCategory::Capture
        );
        assert_eq!(
            dir_category(Path::new("generated/x.typ")),
            NoteCategory::Generated
        );
        assert_eq!(
            dir_category(Path::new("permanent/x.typ")),
            NoteCategory::Permanent
        );
        // defensive only: every real caller hands a path recorded under
        // one of the four category directories
        assert_eq!(
            dir_category(Path::new("elsewhere/x.typ")),
            NoteCategory::Permanent
        );
    }

    #[test]
    fn creating_a_note_through_ctrl_n_indexes_it_with_no_watcher_batch() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, creator_keys) = open_creator(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "concept");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());
        type_into(&mut dom, input, "Watcher Proving Ground");
        press(&mut dom, creator_keys, Key::Enter, Modifiers::empty());

        let index = Index::open(&vault.path().join(".index/index.db"))
            .expect("open the index directly");
        let ids: Vec<String> = index
            .table_notes()
            .expect("table notes")
            .into_iter()
            .map(|note| note.id)
            .collect();
        assert!(
            ids.contains(&"watcher-proving-ground".to_string()),
            "the app indexed its own create before any watcher could: {ids:?}"
        );
    }

    #[test]
    fn opening_the_next_daily_indexes_it_with_no_watcher_batch() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "open next daily");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        let written = vault.path().join("time/2026-07-24.typ");
        assert!(written.exists(), "the missing day was created");

        let index = Index::open(&vault.path().join(".index/index.db"))
            .expect("open the index directly");
        let ids: Vec<String> = index
            .time_notes()
            .expect("time notes")
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(
            ids.contains(&"2026-07-24".to_string()),
            "the app indexed its own time-note create before any watcher \
             could: {ids:?}"
        );
    }

    #[test]
    fn capturing_through_the_chord_indexes_it_with_no_watcher_batch() {
        let vault = temp_vault();
        let (mut dom, _, keys) = capture_app(
            Some(vault.path().to_path_buf()),
            Ok("collé du navigateur".to_string()),
            Some(CAPTURED_AT),
        );
        capture_chord(&mut dom, &keys);

        let index = Index::open(&vault.path().join(".index/index.db"))
            .expect("open the index directly");
        assert_eq!(
            index
                .unsummarized_captures()
                .expect("unsummarized captures"),
            vec![PathBuf::from("capture/capture-2026-07-23-091542.typ")],
            "the app indexed its own capture before any watcher could"
        );
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
        // the fourth character of the heading's first line — markup roles
        // split "= " and "20" into their own spans, so the caret-box still
        // follows the "20" span rather than a merged "= 20" one
        assert!(
            html.contains(r#">20</span><span class="caret-box""#),
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

    // -- Ctrl+Shift+D: the delete chord (adr/2026-08-delete-note-chord.md) --

    #[test]
    fn ctrl_shift_d_deletes_the_open_sheets_note_with_no_confirmation() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);

        // a jittered click: the position writes and the sheet opens
        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (103.0, 98.0));
        mouse(&mut dom, "mouseup", pane, (103.0, 98.0));
        assert!(dioxus_ssr::render(&dom).contains(r#"class="sheet""#));

        press(
            &mut dom,
            keys,
            Key::Character("D".into()),
            Modifiers::CONTROL | Modifiers::SHIFT,
        );

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
    fn ctrl_shift_d_does_nothing_with_no_sheet_open() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        press(
            &mut dom,
            keys,
            Key::Character("D".into()),
            Modifiers::CONTROL | Modifiers::SHIFT,
        );

        assert!(
            vault.path().join("permanent/alpha.typ").exists(),
            "no sheet, nothing to delete"
        );
    }

    #[test]
    fn undo_after_the_delete_chord_restores_the_file_and_its_position() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);

        // a jittered click pins the position before the delete takes it
        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (103.0, 98.0));
        mouse(&mut dom, "mouseup", pane, (103.0, 98.0));
        let path = vault.path().join("permanent/alpha.typ");
        let original =
            std::fs::read_to_string(&path).expect("the note is readable");

        press(
            &mut dom,
            keys,
            Key::Character("D".into()),
            Modifiers::CONTROL | Modifiers::SHIFT,
        );
        assert!(!path.exists(), "the delete landed");

        let (input, palette_keys) = open_palette(&mut dom, keys);
        assert!(
            palette_labels(&dom).contains(&"undo delete alpha".to_string()),
            "{:?}",
            palette_labels(&dom)
        );
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert_eq!(
            std::fs::read_to_string(&path).expect("the note is back"),
            original
        );
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert!(saved.contains("alpha "), "the position returned: {saved}");
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

    // -- the undo register (adr/2026-08-app-level-undo-register.md) ----------

    #[test]
    fn undo_after_delete_restores_the_file_and_its_position() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);

        // a jittered click pins the position before the delete takes it
        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (103.0, 98.0));
        mouse(&mut dom, "mouseup", pane, (103.0, 98.0));
        let path = vault.path().join("permanent/alpha.typ");
        let original =
            std::fs::read_to_string(&path).expect("the note is readable");

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(!path.exists(), "the delete landed");

        // the row wears the register's words, and running it restores the
        // note exactly as it left — text and card both
        let (input, palette_keys) = open_palette(&mut dom, keys);
        assert!(
            palette_labels(&dom).contains(&"undo delete alpha".to_string()),
            "{:?}",
            palette_labels(&dom)
        );
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert_eq!(
            std::fs::read_to_string(&path).expect("the note is back"),
            original
        );
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the debounced write reached the file");
        assert!(saved.contains("alpha "), "the position returned: {saved}");

        // spent: the register is empty and the command hides again
        let _ = open_palette(&mut dom, keys);
        assert!(
            !palette_labels(&dom)
                .iter()
                .any(|label| label.contains("undo")),
            "{:?}",
            palette_labels(&dom)
        );
    }

    /// `undo_last`'s own write seam derives the restored note's category
    /// from its path rather than assuming `Permanent`
    /// (`adr/2026-09-the-app-indexes-its-own-writes.md`) — a capture and a
    /// generated note both take the other two branches. Both deletes run
    /// before either undo, so the fixture's original card elements (keyed
    /// by id, untouched by a *different* card's delete-then-restore) stay
    /// valid throughout — no re-querying the table after a card
    /// disappears and reappears.
    #[test]
    fn undo_after_delete_restores_capture_and_generated_notes() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);

        let capture_path = vault.path().join("capture/capture-idea.typ");
        let capture_original = std::fs::read_to_string(&capture_path)
            .expect("the note is readable");
        open_sheet_on(&mut dom, pane, cards[1]);
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete note");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(!capture_path.exists(), "the capture note was deleted");

        let generated_path = vault.path().join("generated/digest.typ");
        let generated_original = std::fs::read_to_string(&generated_path)
            .expect("the note is readable");
        open_sheet_on(&mut dom, pane, cards[2]);
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete note");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(!generated_path.exists(), "the generated note was deleted");

        // the register pops newest first: digest's delete, then
        // capture-idea's
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert_eq!(
            std::fs::read_to_string(&generated_path)
                .expect("the generated note is back"),
            generated_original,
            "undo restores a generated note"
        );

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert_eq!(
            std::fs::read_to_string(&capture_path)
                .expect("the capture note is back"),
            capture_original,
            "undo restores a capture note"
        );
    }

    /// The fourth branch of the restored note's category: a deleted time
    /// note re-enters the index as `time`, never as a phantom permanent
    /// card the table would draw until a rescan (`dir_category`,
    /// adr/2026-09-the-app-indexes-its-own-writes.md). The sheet it is
    /// deleted from is the unparseable-stem fallback — the one door a
    /// time file has into a sheet.
    #[test]
    fn undo_after_delete_restores_a_time_note_as_time() {
        let vault = temp_vault();
        std::fs::write(vault.path().join("time/notes.typ"), "= sans place\n")
            .expect("the misfiled note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, keys) = table_targets_with_keys(&mut dom, &clicks);

        let (own_keys, _, _) = open_loops_overlay(&mut dom, clicks[EMBER]);
        press(&mut dom, own_keys, Key::Enter, Modifiers::empty());
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete note");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(
            !vault.path().join("time/notes.typ").exists(),
            "the delete landed"
        );

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(
            vault.path().join("time/notes.typ").exists(),
            "the note is back"
        );

        let index =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("open the index directly");
        let category: String = index
            .query_row(
                "SELECT category FROM notes WHERE path = 'time/notes.typ'",
                [],
                |row| row.get(0),
            )
            .expect("the restored note has a row");
        assert_eq!(
            category, "time",
            "the restored note re-entered under its own category"
        );
    }

    #[test]
    fn an_undo_onto_a_recreated_note_refuses_and_reports() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        // the path holds a living file again: the register never clobbers
        let path = vault.path().join("permanent/alpha.typ");
        std::fs::write(&path, "= imposteur\n").expect("the note is reborn");
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        // the bare table carries no notice line; the logs pane does
        click(&mut dom, clicks[CHROME_LOGS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("undo:"), "the refusal surfaced: {html}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("the reborn note stands"),
            "= imposteur\n"
        );
    }

    #[test]
    fn undo_of_an_unpinned_delete_restores_no_position() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);
        open_sheet_on(&mut dom, pane, cards[0]);

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "delete");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());

        assert!(
            vault.path().join("permanent/alpha.typ").exists(),
            "the note is back"
        );
        block_on(settle(&mut dom));
        let saved =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the idle tick still writes the store");
        assert!(
            !saved.contains("alpha"),
            "an unpinned card comes back unpinned: {saved}"
        );
    }

    #[test]
    fn undo_arrange_returns_every_card_it_moved() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            linking(note("beta"), "alpha"),
        )
        .expect("the linking note is written");
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (pane, cards, keys) = table_targets_with_keys(&mut dom, &clicks);

        // alpha pinned by hand, beta auto-placed beside it
        mouse(&mut dom, "mousedown", cards[0], (100.0, 100.0));
        mouse(&mut dom, "mousemove", pane, (103.0, 98.0));
        mouse(&mut dom, "mouseup", pane, (103.0, 98.0));
        block_on(settle(&mut dom));
        let before =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the pin persisted");

        let (input, palette_keys) = open_palette(&mut dom, keys);
        type_into(&mut dom, input, "arrange");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        let arranged =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the arrange persisted");
        assert!(arranged.contains("beta "), "the arrange pinned beta");

        // the reverse: alpha back to its hand-picked spot, beta unpinned
        let (input, palette_keys) = open_palette(&mut dom, keys);
        assert!(
            palette_labels(&dom).contains(&"undo arrange".to_string()),
            "{:?}",
            palette_labels(&dom)
        );
        type_into(&mut dom, input, "undo");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        let after =
            std::fs::read_to_string(vault.path().join(".index/positions"))
                .expect("the undo persisted");
        assert_eq!(after, before, "every card returned: {after}");
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
        // …then the sheet closes under it — the pane's shift+escape, reached
        // directly here, is the race the guard below defends against
        press(&mut dom, keys, Key::Escape, Modifiers::SHIFT);
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
        mounted_app_with_hit(root, closer, None, None)
    }

    fn mounted_app_with_hit(
        root: Option<PathBuf>,
        closer: Option<Closer>,
        hit: Option<HitProbe>,
        line: Option<LineProbe>,
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
        if let Some(line) = line {
            dom.insert_any_root_context(Box::new(line));
        }
        let mutations = dom.rebuild_to_vec();
        (dom, mutations)
    }

    /// A hit cell holding this never answers.
    const HIT_HANGS: (usize, usize) = (usize::MAX, usize::MAX);

    /// Where the scripted hit probe says the next mouse press landed —
    /// (span `data-start`, UTF-16 units within it), or `None` for a miss.
    type HitCell = Arc<std::sync::Mutex<Option<(usize, usize)>>>;

    /// The scripted hit probe and the cell a test steers it with.
    fn hit_probe_fake() -> (HitProbe, HitCell) {
        let landing: HitCell = Arc::new(std::sync::Mutex::new(None));
        let feed = landing.clone();
        let hit = HitProbe(Arc::new(move |_, _| {
            let landed = *feed.lock().expect("the hit cell never poisons");
            if landed == Some(HIT_HANGS) {
                // the probe that stays out, for the one-in-flight guard
                return Box::pin(std::future::pending());
            }
            Box::pin(async move { landed })
        }));
        (hit, landing)
    }

    /// What the recording launcher was handed, in order.
    type Launched = Arc<std::sync::Mutex<Vec<String>>>;

    /// Like `hit_app`, with a launcher injected that records every target
    /// and answers as `verdicts` scripts it, in order — an empty script
    /// answers `Ok` (adr/2026-09-link-is-for-resources.md).
    fn launcher_app(
        root: Option<PathBuf>,
        verdicts: Vec<Result<(), String>>,
    ) -> (VirtualDom, Vec<ElementId>, HitCell, Launched) {
        let launched: Launched = Arc::new(std::sync::Mutex::new(Vec::new()));
        let verdicts = Arc::new(std::sync::Mutex::new(verdicts));
        let recorder = Launcher(Arc::new({
            let launched = launched.clone();
            move |target: &str| {
                launched
                    .lock()
                    .expect("the launch log never poisons")
                    .push(target.to_string());
                let mut verdicts =
                    verdicts.lock().expect("the script never poisons");
                if verdicts.is_empty() {
                    Ok(())
                } else {
                    verdicts.remove(0)
                }
            }
        }));
        let (hit, landing) = hit_probe_fake();
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(test_today())));
        dom.insert_any_root_context(Box::new(hit));
        dom.insert_any_root_context(Box::new(recorder));
        let mutations = dom.rebuild_to_vec();
        let clicks = listeners(&mutations, "click");
        (dom, clicks, landing, launched)
    }

    /// Like `rendered_app`, but with a scripted hit probe injected: each
    /// mouse press reads whatever the returned cell holds at that moment.
    fn hit_app(
        root: Option<PathBuf>,
    ) -> (VirtualDom, Vec<ElementId>, HitCell) {
        let (hit, landing) = hit_probe_fake();
        let (dom, mutations) =
            mounted_app_with_hit(root, None, Some(hit), None);
        let clicks = listeners(&mutations, "click");
        (dom, clicks, landing)
    }

    /// The drawn lines the scripted line probe has below (or above) the
    /// caret, one entry per step: `(data-start, UTF-16 units, pixel x)`.
    /// An empty script is the seam with nothing to answer.
    type LineScript = Arc<std::sync::Mutex<Vec<(usize, usize, f64)>>>;

    /// What the scripted line probe was asked for, once per press: the
    /// goal x it was handed and the number of steps the run wanted.
    type LineAsks = Arc<std::sync::Mutex<Vec<(Option<f64>, usize)>>>;

    /// Like `hit_app`, but scripts the line probe the way the real one
    /// behaves: one round trip per press, walking as many of the asked
    /// steps as the script has drawn lines and answering the landing it
    /// reached — so a count of three is visible as three steps consumed,
    /// and a script shorter than the count is the drawn extent running out
    /// (adr/2026-08-visual-line-j-k-through-a-geometry-seam.md).
    fn line_app(
        root: Option<PathBuf>,
    ) -> (VirtualDom, Vec<ElementId>, LineScript, LineAsks) {
        let (line, script, asked) = line_probe_fake(None);
        let (dom, mutations) =
            mounted_app_with_hit(root, None, None, Some(line));
        let clicks = listeners(&mutations, "click");
        (dom, clicks, script, asked)
    }

    /// `line_app` whose first walk hangs until the returned sender fires:
    /// the only way to have a walk still out when the next key lands, and
    /// so the only way to reach the run's generation guard.
    #[allow(clippy::type_complexity)]
    fn latched_line_app(
        root: Option<PathBuf>,
    ) -> (
        VirtualDom,
        Vec<ElementId>,
        LineScript,
        LineAsks,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (release, gate) = tokio::sync::oneshot::channel();
        let (line, script, asked) = line_probe_fake(Some(gate));
        let (dom, mutations) =
            mounted_app_with_hit(root, None, None, Some(line));
        let clicks = listeners(&mutations, "click");
        (dom, clicks, script, asked, release)
    }

    /// `line_app` with the scripted hit probe beside the line one: what
    /// the mouse does to a j/k run's held goal column is only visible
    /// when both seams answer.
    #[allow(clippy::type_complexity)]
    fn line_and_hit_app(
        root: Option<PathBuf>,
    ) -> (VirtualDom, Vec<ElementId>, LineScript, LineAsks, HitCell) {
        let (line, script, asked) = line_probe_fake(None);
        let (hit, landing) = hit_probe_fake();
        let (dom, mutations) =
            mounted_app_with_hit(root, None, Some(hit), Some(line));
        let clicks = listeners(&mutations, "click");
        (dom, clicks, script, asked, landing)
    }

    /// The scripted line probe and the two cells a test steers and reads
    /// it with. `gate`, when given, hangs the very first walk until it
    /// fires — every later walk answers at once.
    fn line_probe_fake(
        gate: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> (LineProbe, LineScript, LineAsks) {
        let script: LineScript = Arc::new(std::sync::Mutex::new(Vec::new()));
        let feed = script.clone();
        let asked: LineAsks = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = asked.clone();
        let latch = Arc::new(std::sync::Mutex::new(gate));
        let line = LineProbe(Arc::new(move |goal, _down, count| {
            recorder
                .lock()
                .expect("the goal cell never poisons")
                .push((goal, count));
            let held = latch.lock().expect("the latch never poisons").take();
            let drawn = feed.lock().expect("the line cell never poisons");
            let taken = count.min(drawn.len());
            let landed = taken.checked_sub(1).map(|last| Landing {
                start: drawn[last].0,
                units: drawn[last].1,
                x: drawn[last].2,
                taken,
            });
            Box::pin(async move {
                if let Some(held) = held {
                    let _ = held.await;
                }
                landed
            })
        }));
        (line, script, asked)
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
        press(dom, sink, Key::Character("i".into()), Modifiers::empty());
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
        pasted: Result<String, String>,
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
        pasted: Result<String, String>,
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

    /// The app with a scripted text read, a scripted image read and a
    /// fixed clock — the three seams an image paste crosses.
    fn image_app(
        root: Option<PathBuf>,
        pasted: Result<String, String>,
        image: Result<Option<Vec<u8>>, String>,
    ) -> (VirtualDom, Vec<ElementId>) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(test_today())));
        dom.insert_any_root_context(Box::new(Clipboard(Arc::new(
            move || {
                let pasted = pasted.clone();
                Box::pin(async move { pasted })
            },
        ))));
        dom.insert_any_root_context(Box::new(ClipboardImage(Arc::new(
            move || {
                let image = image.clone();
                Box::pin(async move { image })
            },
        ))));
        let stamp: jiff::Zoned = "2026-07-23T10:15:00[UTC]"
            .parse()
            .expect("the paste clock is a valid timestamp");
        dom.insert_any_root_context(Box::new(Now(Arc::new(move || {
            stamp.clone()
        }))));
        let mutations = dom.rebuild_to_vec();
        let clicks = listeners(&mutations, "click");
        (dom, clicks)
    }

    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 13, 10, 26, 10];

    #[test]
    fn p_over_an_image_and_no_text_files_the_png_and_spells_the_image() {
        let vault = temp_vault();
        std::fs::create_dir_all(vault.path().join("assets"))
            .expect("the assets dir");
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Ok(String::new()),
            Ok(Some(PNG_SIGNATURE.to_vec())),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let name = "2026-07-23-20260723-101500.png";
        assert_eq!(
            std::fs::read(vault.path().join("assets").join(name))
                .expect("the png landed"),
            PNG_SIGNATURE.to_vec()
        );
        assert!(
            source_of(&dom).contains(&format!("#image(\"/assets/{name}\")")),
            "{}",
            source_of(&dom)
        );
    }

    #[test]
    fn ctrl_v_in_insert_mode_pastes_the_image_call_at_the_caret() {
        let vault = temp_vault();
        std::fs::create_dir_all(vault.path().join("assets"))
            .expect("the assets dir");
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Ok(String::new()),
            Ok(Some(PNG_SIGNATURE.to_vec())),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("i".into()),
            Modifiers::empty(),
        );
        press(
            &mut dom,
            sink,
            Key::Character("v".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        assert!(
            source_of(&dom).contains("#image(\"/assets/2026-07-23-"),
            "{}",
            source_of(&dom)
        );
        // text on the clipboard still wins over an image
        let vault = temp_vault();
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Ok("texte".to_string()),
            Ok(Some(PNG_SIGNATURE.to_vec())),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(source_of(&dom).contains("texte"), "{}", source_of(&dom));
        assert!(!source_of(&dom).contains("#image"), "{}", source_of(&dom));
    }

    #[test]
    fn a_refused_text_read_still_pastes_the_image_and_is_reported_only_alone()
    {
        // X11 refuses the text read when the clipboard holds only an image
        let vault = temp_vault();
        std::fs::create_dir_all(vault.path().join("assets"))
            .expect("the assets dir");
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Err("no text target".to_string()),
            Ok(Some(PNG_SIGNATURE.to_vec())),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(source_of(&dom).contains("#image("), "{}", source_of(&dom));
        assert!(!dioxus_ssr::render(&dom).contains("clipboard: "));

        // and with no image behind the refusal, the refusal is the notice
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Err("no text target".to_string()),
            Ok(None),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom).contains("clipboard: no text target"),
            "{}",
            dioxus_ssr::render(&dom)
        );
    }

    #[test]
    fn an_image_read_that_finds_nothing_refuses_or_cannot_be_filed_pastes_nothing()
     {
        let vault = temp_vault();
        // nothing on the clipboard at all
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Ok(String::new()),
            Ok(None),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before);
        assert!(!dioxus_ssr::render(&dom).contains("notice-warning"));

        // the image read refused
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Ok(String::new()),
            Err("no image target".to_string()),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("clipboard: no image target"), "{html}");

        // the assets directory is not there to write into
        let (mut dom, clicks) = image_app(
            Some(vault.path().to_path_buf()),
            Ok(String::new()),
            Ok(Some(PNG_SIGNATURE.to_vec())),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);
        press(
            &mut dom,
            sink,
            Key::Character("p".into()),
            Modifiers::empty(),
        );
        block_on(settle(&mut dom));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("image: "), "the write refused aloud: {html}");
        assert!(html.contains("nothing was pasted"), "{html}");
        assert_eq!(source_of(&dom), before);
    }

    /// A sequence of clipboard outcomes for recovery assertions.
    fn clipboard_script_app(
        root: Option<PathBuf>,
        pasted: VecDeque<Result<String, String>>,
    ) -> (VirtualDom, Vec<ElementId>) {
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        let pasted = Arc::new(std::sync::Mutex::new(pasted));
        dom.insert_any_root_context(Box::new(Clipboard(Arc::new(
            move || {
                let answer = pasted
                    .lock()
                    .expect("the clipboard script")
                    .pop_front()
                    .unwrap_or_else(|| {
                        Err("clipboard script exhausted".to_string())
                    });
                Box::pin(async move { answer })
            },
        ))));
        let mutations = dom.rebuild_to_vec();
        (dom, listeners(&mutations, "click"))
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
    /// zero-width caret span contributes nothing. Scoped to the one
    /// `.block-active` div: every other block now renders its own
    /// `.source-line` divs too (`Pane::Css`,
    /// adr/2026-08-css-draws-the-markup.md), so reading every
    /// `.source-line` in the page would run the whole note's rendered text
    /// together instead of just the active block's.
    fn source_of(dom: &VirtualDom) -> String {
        let html = dioxus_ssr::render(dom);
        // a prefix, not the whole `class` value: the active block now
        // carries its markup block role too
        // (adr/2026-08-css-draws-the-markup.md), so the div's class
        // attribute is no longer exactly `"block-active"`
        let active = html
            .split(r#"<div class="block-active"#)
            .nth(1)
            .and_then(|rest| rest.split(r#"<input class="ime-sink""#).next())
            .unwrap_or("");
        let lines: Vec<String> = active
            .split(r#"<div class="source-line">"#)
            .skip(1)
            .filter_map(|rest| rest.split("</div>").next())
            .map(|line| {
                // the composition preview is drawn but not buffer content —
                // a prefix, since the span now also carries its markup role
                let line: String = line
                    .split(r#"<span class="compose"#)
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
                // the box caret's end-of-line stand-in space is drawn but
                // not buffer content; a real cluster under the box is
                let line: String = line
                    .split(r#"<span class="caret-box"#)
                    .enumerate()
                    .map(|(index, part)| {
                        if index == 0 {
                            return part.to_string();
                        }
                        let (boxed, rest) =
                            part.split_once("</span>").unwrap_or((part, ""));
                        let cluster = boxed
                            .split_once('>')
                            .map(|(_, inner)| inner)
                            .unwrap_or("");
                        if cluster == "\u{a0}" {
                            rest.to_string()
                        } else {
                            format!("{cluster}{rest}")
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
            .replace("&#62;", ">")
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

    /// Opens the finder with Ctrl+Shift+F from `keys` and returns its
    /// (input, keydown) targets; the rows come with the hits, after typing.
    fn open_finder(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, ElementId) {
        let mutations = press_for_mutations(
            dom,
            keys,
            Key::Character("F".into()),
            Modifiers::CONTROL | Modifiers::SHIFT,
        );
        let inputs = listeners(&mutations, "input");
        let keydowns = listeners(&mutations, "keydown");
        mount(dom, listeners(&mutations, "mounted")[0]);
        (inputs[0], keydowns[0])
    }

    /// `type_into`, handing back the mutations the input caused — the
    /// finder's rows mount on them.
    fn type_for_mutations(
        dom: &mut VirtualDom,
        target: ElementId,
        text: &str,
    ) -> Mutations {
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
            dom.render_immediate_to_vec()
        })
    }

    #[test]
    fn ctrl_shift_f_finds_a_word_past_the_preamble_and_enter_opens_the_sheet()
    {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("permanent/beta.typ"),
            format!("{}the quick brown fox\n", note("beta")),
        )
        .expect("beta is written");
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, finder_keys) = open_finder(&mut dom, keys[LOGS_KEYS]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">search text<"), "the finder opened: {html}");
        assert!(html.contains("type to search the vault"), "{html}");
        // Enter over no hit does nothing, and a ctrl chord passes through
        press(&mut dom, finder_keys, Key::Enter, Modifiers::empty());
        press(&mut dom, finder_keys, ctrl_p(), Modifiers::CONTROL);
        assert!(dioxus_ssr::render(&dom).contains(">search text<"));

        type_into(&mut dom, input, "brown");
        let html = dioxus_ssr::render(&dom);
        assert_eq!(picker_ids(&dom), ["beta"], "{html}");
        assert!(html.contains("quick brown fox"), "the snippet: {html}");
        // a key the finder has no arm for is absorbed by the input
        press(
            &mut dom,
            finder_keys,
            Key::Character("x".into()),
            Modifiers::empty(),
        );
        assert_eq!(picker_ids(&dom), ["beta"]);
        // a note with no heading is listed under its stem
        std::fs::write(
            vault.path().join("permanent/untitled.typ"),
            "#import \"/templates/template.typ\": *\n#show: note\n\
             #meta(id: \"untitled\", type: \"concept\")\n\nwallaby words\n",
        )
        .expect("the untitled note is written");
        crate::compute::refresh(
            vault.path(),
            &[watch::VaultChange::Rescan],
            test_today(),
        )
        .expect("the vault re-indexes");
        type_into(&mut dom, input, "wallaby");
        assert_eq!(picker_ids(&dom), ["untitled"]);
        // the preamble is not searched: every note imports the template
        type_into(&mut dom, input, "template");
        assert!(dioxus_ssr::render(&dom).contains("no matching note"));

        type_into(&mut dom, input, "brown");
        press(&mut dom, finder_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(">search text<"), "the finder closed: {html}");
        assert!(html.contains(r#"class="sheet""#), "beta's sheet: {html}");
    }

    #[test]
    fn a_time_note_hit_lands_on_the_logs_and_a_row_click_lands_too() {
        let vault = temp_vault();
        std::fs::write(
            vault.path().join("time/2026-07-22.typ"),
            format!("{}zebra crossing\n", time_note("2026-07-22", "daily")),
        )
        .expect("the day is rewritten");
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, finder_keys) = open_finder(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "zebra");
        assert_eq!(picker_ids(&dom), ["2026-07-22"]);
        // arrows clamp at both ends of the one row
        press(&mut dom, finder_keys, Key::ArrowDown, Modifiers::empty());
        press(&mut dom, finder_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, finder_keys, Key::ArrowUp, Modifiers::empty());
        press(&mut dom, finder_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "the day is selected: {html}"
        );

        // the same landing from a click on the row, and Escape closes
        let (input, finder_keys) = open_finder(&mut dom, keys[LOGS_KEYS]);
        let typed = type_for_mutations(&mut dom, input, "zebra");
        let rows = listeners(&typed, "click");
        click(&mut dom, rows[0]);
        assert!(!dioxus_ssr::render(&dom).contains(">search text<"));
        let (_input, finder_keys_again) =
            open_finder(&mut dom, keys[LOGS_KEYS]);
        let _ = finder_keys;
        press(&mut dom, finder_keys_again, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">search text<"));
    }

    #[test]
    fn a_search_the_index_cannot_answer_is_a_notice() {
        let vault = temp_vault();
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let saboteur =
            rusqlite::Connection::open(vault.path().join(".index/index.db"))
                .expect("a second connection opens");
        saboteur
            .execute_batch("DROP TABLE notes_fts")
            .expect("the sabotage succeeds");
        let (input, _) = open_finder(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "anything");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("search: "), "the read failed aloud: {html}");
        assert!(html.contains("no matching note"), "{html}");

        // an index that will not open at all says the same
        replace_database_with_a_directory(vault.path());
        type_into(&mut dom, input, "anything else");
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("search: "), "{html}");
    }

    #[test]
    fn the_finder_opens_from_the_table_and_from_the_palette() {
        let vault = temp_vault();
        let (mut dom, clicks, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, _, table_keys) = table_targets_with_keys(&mut dom, &clicks);
        let (_input, finder_keys) = open_finder(&mut dom, table_keys);
        assert!(dioxus_ssr::render(&dom).contains(">search text<"));
        // a second chord over the open finder is inert: overlays never stack
        press(
            &mut dom,
            table_keys,
            Key::Character("F".into()),
            Modifiers::CONTROL | Modifiers::SHIFT,
        );
        press(&mut dom, finder_keys, Key::Escape, Modifiers::empty());
        assert!(!dioxus_ssr::render(&dom).contains(">search text<"));

        // Ctrl+F alone is still the card filter, not the finder
        press(
            &mut dom,
            table_keys,
            Key::Character("f".into()),
            Modifiers::CONTROL,
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(">filter<"), "{html}");
        assert!(!html.contains(">search text<"), "{html}");

        // and the palette row runs it from the logs
        let _ = keys;
        let (mut dom, _, keys, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, palette_keys) = open_palette(&mut dom, keys[LOGS_KEYS]);
        type_into(&mut dom, input, "search text");
        press(&mut dom, palette_keys, Key::Enter, Modifiers::empty());
        assert!(dioxus_ssr::render(&dom).contains(">search text<"));
    }

    /// Opens the Ctrl+O note switcher and returns its input's (input,
    /// keydown) targets plus its rows' click targets in list order —
    /// `open_palette`'s shape over the one switcher
    /// (adr/2026-09-ctrl-o-is-the-one-note-switcher.md).
    fn open_switcher(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, ElementId, Vec<ElementId>) {
        let mutations =
            press_for_mutations(dom, keys, ctrl_o(), Modifiers::CONTROL);
        let inputs = listeners(&mutations, "input");
        let keydowns = listeners(&mutations, "keydown");
        let rows = listeners(&mutations, "click");
        mount(dom, listeners(&mutations, "mounted")[0]);
        (inputs[0], keydowns[0], rows)
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

    /// Runs "edit template" from the palette and returns the template
    /// picker's (input, keydown) targets from the dispatch's mutations —
    /// `open_palette`, one overlay deeper.
    fn open_template_picker(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, ElementId) {
        let (input, palette_keys) = open_palette(dom, keys);
        type_into(dom, input, "edit template");
        let opened = press_for_mutations(
            dom,
            palette_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let inputs = listeners(&opened, "input");
        let keydowns = listeners(&opened, "keydown");
        mount(dom, listeners(&opened, "mounted")[0]);
        (inputs[0], keydowns[0])
    }

    /// Opens the settings overlay with Ctrl+, and returns its own keydown
    /// target (the overlay's Escape rung, defence in depth behind the
    /// pane's ladder) and its click targets in markup order — the theme
    /// button, then the font-size stepper's minus and plus
    /// (adr/2026-08-settings-overlay.md).
    fn open_settings_overlay(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, Vec<ElementId>) {
        let mutations = press_for_mutations(
            dom,
            keys,
            Key::Character(",".into()),
            Modifiers::CONTROL,
        );
        let keydown = listeners(&mutations, "keydown")[0];
        let clicks = listeners(&mutations, "click");
        mount(dom, listeners(&mutations, "mounted")[0]);
        (keydown, clicks)
    }

    /// Opens the notices overlay through the palette's "notices" command
    /// and returns its own (keydown, click) targets — the pane's own
    /// escape and click-to-close, tested apart from the app-level ladder
    /// rung that stays as a fallback
    /// (adr/2026-08-palette-order-and-overlay-placement.md).
    fn open_notices_overlay(
        dom: &mut VirtualDom,
        keys: ElementId,
    ) -> (ElementId, ElementId) {
        let (input, palette_keys) = open_palette(dom, keys);
        type_into(dom, input, "notices");
        let mutations = press_for_mutations(
            dom,
            palette_keys,
            Key::Enter,
            Modifiers::empty(),
        );
        let keydown = listeners(&mutations, "keydown")[0];
        let click = listeners(&mutations, "click")[0];
        mount(dom, listeners(&mutations, "mounted")[0]);
        (keydown, click)
    }

    /// Opens the loops overlay through the ember and returns its own
    /// (keydown, click) targets — the same self-contained escape and
    /// click-to-close as the notices pane
    /// (adr/2026-08-palette-order-and-overlay-placement.md) — plus its
    /// rows' click targets in list order, the back picker's shape
    /// (`open_back_picker`) now that a row opens its note
    /// (adr/2026-09-loop-lines-open-their-notes.md). The container's own
    /// click listener registers before its children's, so it is the first
    /// of the batch and the rows are everything after it.
    fn open_loops_overlay(
        dom: &mut VirtualDom,
        ember: ElementId,
    ) -> (ElementId, ElementId, Vec<ElementId>) {
        let mutations = click_for_mutations(dom, ember);
        let keydown = listeners(&mutations, "keydown")[0];
        let clicks = listeners(&mutations, "click");
        let click = clicks[0];
        let rows = clicks[1..].to_vec();
        mount(dom, listeners(&mutations, "mounted")[0]);
        (keydown, click, rows)
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

    /// The heading widget's targets: the fixture day note's own heading
    /// block click, direct — every block above the active trailing line
    /// wakes on its own click now, no merged region to navigate out of
    /// (adr/2026-08-css-draws-the-markup.md).
    fn activate_heading(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, ElementId) {
        activate_block(dom, clicks[BLOCK_HEADING])
    }

    /// The link line's targets: the fixture day note's own link-block
    /// click, direct.
    fn activate_link(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, ElementId) {
        activate_block(dom, clicks[BLOCK_LINK])
    }

    /// The preamble's targets: the fixture day note's own preamble-block
    /// click, direct.
    fn activate_preamble(
        dom: &mut VirtualDom,
        clicks: &[ElementId],
    ) -> (ElementId, ElementId) {
        activate_block(dom, clicks[BLOCK_PREAMBLE])
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

    /// Wakes the sheet's heading directly. Opening the sheet leaves the
    /// note's own trailing empty line active
    /// (adr/2026-08-cursor-always-in-the-note.md); every block above it
    /// gets its own click listener, in document order — alpha's fixture is
    /// preamble, blank, heading, so the heading is the third
    /// (adr/2026-08-css-draws-the-markup.md).
    fn sheet_heading_targets(
        dom: &mut VirtualDom,
        opened: &Mutations,
    ) -> (ElementId, ElementId) {
        activate_block(dom, listeners(opened, "click")[2])
    }

    /// Like `sheet_heading_targets`, but for a note whose heading is
    /// followed by a link line: preamble, blank, heading, link — the link
    /// is the fourth click.
    fn sheet_link_targets(
        dom: &mut VirtualDom,
        opened: &Mutations,
    ) -> (ElementId, ElementId) {
        activate_block(dom, listeners(opened, "click")[3])
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
                format!("{}[[ghost]]\n", note("linky")),
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

    /// Saves are refused by locking the note's *directory*: an atomic
    /// write never opens the target file, it creates a sibling and renames
    /// — so only the directory can say no (`persist::write_atomic`).
    /// Callers unlock before the tempdir drops, or its cleanup would leak.
    fn lock_dir(dir: &std::path::Path, readonly: bool) {
        let mut permissions = std::fs::metadata(dir)
            .expect("the dir exists")
            .permissions();
        permissions.set_readonly(readonly);
        std::fs::set_permissions(dir, permissions)
            .expect("the dir permissions are set");
    }

    /// An external edit the guard must notice: rewrite the file and push
    /// its mtime to the epoch, so the divergence never hides inside the
    /// filesystem's timestamp granularity.
    fn edit_behind(file: &std::path::Path, text: &str) {
        std::fs::write(file, text).expect("the outside edit is written");
        std::fs::OpenOptions::new()
            .write(true)
            .open(file)
            .expect("the note reopens for backdating")
            .set_modified(std::time::SystemTime::UNIX_EPOCH)
            .expect("the mtime is set");
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
        format!("{note}[[{target}]]\n")
    }

    fn note(id: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{id}\", type: \"concept\", created: \"2026-07-01\")\n\
             \n= {id}\n"
        )
    }

    /// The note body `the_logs_and_the_sheet_render_one_note_the_same_way`
    /// opens through both hosts, identical byte for byte in both
    /// categories: a heading, a bold/italic run and a checklist item, so
    /// the comparison below exercises more than the preamble's own
    /// fallback SVG agreeing with itself (`mk-h1`, `mk-strong`, `mk-emph`,
    /// `mk-item`, `mk-checkbox`).
    fn twin_body() -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{TODAY}\", type: \"concept\", created: \"2026-07-01\")\n\
             \n= {TODAY}\n\
             \n*bold* and _emph_ text\n\
             \n- [ ] a todo\n"
        )
    }

    /// A vault holding `twin_body()` at both `time/{TODAY}.typ` — the file
    /// `Shell`'s daily-note hook opens by a bare filesystem stat, no index
    /// lookup needed — and `permanent/{TODAY}.typ` — the file the table
    /// cards from its index scan. Same id, same content, two categories:
    /// the only way to open literally the same note through both the logs
    /// screen and the sheet, since neither host can open the other's
    /// category (the logs pane is date-scoped; the table only cards
    /// non-time notes, `table.rs`'s `label`).
    fn twin_vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let template = concat!(
            "#let meta(id: none, type: none, created: none, ",
            "tags: (), origin: none) = []\n",
            "#let note(doc) = doc\n",
        );
        let body = twin_body();
        for (path, text) in [
            ("templates/template.typ".to_string(), template.to_string()),
            (format!("time/{TODAY}.typ"), body.clone()),
            (format!("permanent/{TODAY}.typ"), body),
        ] {
            let full = dir.path().join(&path);
            std::fs::create_dir_all(
                full.parent().expect("vault files sit in a category"),
            )
            .expect("the category directory is created");
            std::fs::write(full, text).expect("the note is written");
        }
        dir
    }

    /// The `<div class="note-blocks">…</div>` subtree of one whole page's
    /// SSR output, matched by scanning `<div`/`</div>` tags in order and
    /// tracking nesting depth — bounded by the page's own length, since the
    /// house bans `while` and a rendered page has no other honest bound to
    /// give a `for` loop.
    fn note_blocks_subtree(html: &str) -> &str {
        let start = html
            .find(r#"<div class="note-blocks""#)
            .expect("blocks_view always mounts note-blocks once opened");
        let mut depth = 0usize;
        let mut cursor = start;
        for _ in 0..html.len() {
            let next_open = html[cursor..].find("<div").map(|at| cursor + at);
            let next_close =
                html[cursor..].find("</div>").map(|at| cursor + at);
            match (next_open, next_close) {
                (Some(open), Some(close)) if open < close => {
                    depth += 1;
                    cursor = open + "<div".len();
                }
                (_, Some(close)) => {
                    depth -= 1;
                    cursor = close + "</div>".len();
                    if depth == 0 {
                        return &html[start..cursor];
                    }
                }
                _ => break,
            }
        }
        panic!("note-blocks never closes: {html}");
    }

    /// The two editor hosts share one `blocks_view` closure over the one
    /// editor signal (adr/2026-08-sheet-reuses-the-one-editor.md) and are
    /// each wrapped in its own reading column in `assets/theme.css` — the
    /// logs' fluid `.centre-column` capped at 720px
    /// (adr/2026-09-the-logs-column-is-720px.md), the sheet's
    /// `.sheet-column` filling the index card that already bounds it
    /// (adr/2026-09-the-sheet-is-an-index-card.md) — this opens the
    /// identical note body through both (`twin_vault`, the only way to
    /// reach the same content from both hosts, since the logs pane is
    /// date-scoped and the table only cards non-time notes) and checks the
    /// architecture actually holds: the `note-blocks` subtree comes out
    /// byte for byte the same, and each host wraps it in its own
    /// reading-column class exactly once, so a regression that
    /// special-cases one host, or that drops a wrapper, fails loud.
    #[test]
    fn the_logs_and_the_sheet_render_one_note_the_same_way() {
        let vault = twin_vault();
        let (mut dom, clicks, ..) =
            rendered_app(Some(vault.path().to_path_buf()));

        let logs_html = dioxus_ssr::render(&dom);
        let logs_blocks = note_blocks_subtree(&logs_html);
        assert_eq!(
            logs_html.matches(r#"class="centre-column""#).count(),
            1,
            "{logs_html}"
        );
        assert!(
            logs_html.find(r#"class="centre-column""#)
                < logs_html.find(r#"class="note-blocks""#),
            "the reading column wraps note-blocks, not the reverse"
        );

        let (pane, cards) = table_targets(&mut dom, &clicks);
        assert_eq!(cards.len(), 1, "one twin note, one card");
        open_sheet_on(&mut dom, pane, cards[0]);
        let sheet_html = dioxus_ssr::render(&dom);
        let sheet_blocks = note_blocks_subtree(&sheet_html);
        assert_eq!(
            sheet_html.matches(r#"class="sheet-column""#).count(),
            1,
            "{sheet_html}"
        );
        assert!(
            sheet_html.find(r#"class="sheet-column""#)
                < sheet_html.find(r#"class="note-blocks""#),
            "the reading column wraps note-blocks, not the reverse"
        );

        assert_eq!(
            logs_blocks, sheet_blocks,
            "same note, same spans, same classes, same data-start, same order"
        );
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

    // -- gf, the ex prompt and the scroll anchor -----------------------------

    /// adr/2026-08-gf-follows-the-link.md
    #[test]
    fn gf_follows_the_link_under_the_caret() {
        let vault = temp_vault();
        let (mut dom, clicks, hit) = hit_app(Some(vault.path().to_path_buf()));
        let (block, keys) = activate_link(&mut dom, &clicks);

        // the same landing Ctrl+Enter reaches, spelled the way the user's
        // own vim spells it
        place_caret(&mut dom, block, &hit, IN_LINK);
        for key in ["g", "f"] {
            press(
                &mut dom,
                keys,
                Key::Character(key.into()),
                Modifiers::empty(),
            );
        }
        let html = dioxus_ssr::render(&dom);
        assert!(
            html.contains("cal-day has-note selected\">22"),
            "gf jumped to the linked day: {html}"
        );
    }

    /// adr/2026-08-ex-line-is-literal-and-global.md
    #[test]
    fn the_ex_prompt_substitutes_and_one_undo_takes_it_back() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        assert!(
            source_of(&dom).contains("2026-07-23"),
            "{}",
            source_of(&dom)
        );

        // : opens the same one-line prompt / uses, in the same region,
        // wearing the other sigil (AIR LAY-1)
        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character(":".into()),
            Modifiers::empty(),
        );
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"placeholder=":""#), "{html}");
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);

        type_into(&mut dom, prompt_input, "s/2026/2027/");
        press(&mut dom, prompt_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"placeholder=":""#), "the prompt closed");
        assert!(
            source_of(&dom).contains("2027-07-23"),
            "{}",
            source_of(&dom)
        );

        // one splice, so one u reverses the whole substitution (AIR ACT-2).
        // The click that woke the heading never dirtied the text, so its
        // checkpoint deduped away and undo falls back to the file-open one,
        // waking that snapshot's own last block rather than the heading
        // (adr/2026-08-undo-at-vim-grain.md) — the note's text is what the
        // press is about either way.
        press(
            &mut dom,
            sink,
            Key::Character("u".into()),
            Modifiers::empty(),
        );
        assert!(
            !dioxus_ssr::render(&dom).contains("2027"),
            "one press took the whole substitution back"
        );
    }

    #[test]
    fn escape_closes_the_ex_prompt_leaving_the_note_untouched() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);

        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character(":".into()),
            Modifiers::empty(),
        );
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);
        type_into(&mut dom, prompt_input, "s/2026/2027/");
        press(&mut dom, prompt_keys, Key::Escape, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(!html.contains(r#"placeholder=":""#), "the prompt closed");
        assert_eq!(source_of(&dom), before, "and changed nothing (AIR ERR-6)");

        // an unrelated key inside the prompt is neither a commit nor a
        // cancel: the prompt owns it and stays open
        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character(":".into()),
            Modifiers::empty(),
        );
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);
        press(&mut dom, prompt_keys, Key::ArrowLeft, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"placeholder=":""#), "still open: {html}");
    }

    /// A submitted line that cannot be read speaks through the status
    /// surface rather than vanishing (AIR ERR-2,
    /// adr/2026-08-status-surface-owns-notices.md).
    #[test]
    fn an_unreadable_ex_line_reaches_the_status_surface() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character(":".into()),
            Modifiers::empty(),
        );
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);
        type_into(&mut dom, prompt_input, "nope");
        press(&mut dom, prompt_keys, Key::Enter, Modifiers::empty());
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("unknown command"), "{html}");
        assert!(html.contains(":w saves"), "and says what to type: {html}");
    }

    #[test]
    fn the_ex_line_writes_the_note_to_disk_on_demand() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        // type into the buffer, then :w rather than waiting for the pause
        for key in ["A", "!"] {
            press(
                &mut dom,
                sink,
                Key::Character(key.into()),
                Modifiers::empty(),
            );
        }
        press(&mut dom, sink, Key::Escape, Modifiers::empty());
        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character(":".into()),
            Modifiers::empty(),
        );
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);
        type_into(&mut dom, prompt_input, "w");
        press(&mut dom, prompt_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));

        let on_disk =
            std::fs::read_to_string(vault.path().join("time/2026-07-23.typ"))
                .expect("the day note is on disk");
        assert!(on_disk.contains("2026-07-23!"), "{on_disk}");
    }

    /// adr/2026-08-visual-gains-p-r-s-and-gv.md
    #[test]
    fn visual_p_replaces_the_selection_from_the_clipboard() {
        let vault = temp_vault();
        let (mut dom, clicks, written) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Ok("collée".to_string()),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        for key in ["v", "l", "l", "p"] {
            press(
                &mut dom,
                sink,
                Key::Character(key.into()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));
        assert!(source_of(&dom).contains("collée"), "{}", source_of(&dom));
        assert!(
            written.lock().expect("the write cell").is_empty(),
            "what it replaced never went back out to the clipboard"
        );
    }

    /// adr/2026-08-scroll-anchor-is-consumed-once.md. The headless DOM
    /// cannot read a `ScrollLogicalPosition`, but it can see the caret
    /// span remount — which is the whole mechanism: the span's key carries
    /// the nonce, so a `zz` that moves the caret not at all still gets a
    /// fresh mount to scroll from.
    #[test]
    fn each_scroll_anchor_remounts_the_caret_and_the_mount_consumes_it() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);

        for anchor in ["z", "t", "b"] {
            // z alone only arms the prefix: nothing has moved yet
            let armed = press_for_mutations(
                &mut dom,
                sink,
                Key::Character("z".into()),
                Modifiers::empty(),
            );
            assert!(
                listeners(&armed, "mounted").is_empty(),
                "z{anchor}: the bare z scrolls nothing",
            );
            let asked = press_for_mutations(
                &mut dom,
                sink,
                Key::Character(anchor.into()),
                Modifiers::empty(),
            );
            let mounts = listeners(&asked, "mounted");
            assert!(!mounts.is_empty(), "z{anchor} remounted the caret");
            // the mount consumes the anchor and falls back to Nearest, so
            // the async fragment landings that remount this same span
            // later move nothing (AIR LAY-2)
            mount(&mut dom, mounts[0]);
            block_on(settle(&mut dom));
        }

        // this editor has no folds: za asks for no scroll at all
        press(
            &mut dom,
            sink,
            Key::Character("z".into()),
            Modifiers::empty(),
        );
        let inert = press_for_mutations(
            &mut dom,
            sink,
            Key::Character("a".into()),
            Modifiers::empty(),
        );
        assert!(listeners(&inert, "mounted").is_empty(), "za moved nothing",);
    }

    #[test]
    fn a_refused_ex_write_says_so_instead_of_failing_quietly() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (input, sink) = activate_heading(&mut dom, &clicks);
        type_into(&mut dom, input, "= pas encore sauvé\n");

        lock_dir(&vault.path().join("time"), true);
        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character(":".into()),
            Modifiers::empty(),
        );
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);
        type_into(&mut dom, prompt_input, "w");
        press(&mut dom, prompt_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        lock_dir(&vault.path().join("time"), false);

        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("notice-critical"), "{html}");
        assert!(
            html.contains("2026-07-23.typ"),
            "the refusal names the file: {html}"
        );
    }

    #[test]
    fn an_ex_line_submitted_with_no_note_open_does_nothing() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let opened = press_for_mutations(
            &mut dom,
            sink,
            Key::Character(":".into()),
            Modifiers::empty(),
        );
        let prompt_input = listeners(&opened, "input")[0];
        let prompt_keys = listeners(&opened, "keydown")[0];
        mount(&mut dom, listeners(&opened, "mounted")[0]);
        type_into(&mut dom, prompt_input, "s/2026/2027/");

        // the prompt owns every plain key but lets the ctrl chords bubble,
        // so the screen can change out from under an open line
        press(
            &mut dom,
            prompt_keys,
            Key::Character("1".into()),
            Modifiers::CONTROL,
        );
        block_on(settle(&mut dom));
        press(&mut dom, prompt_keys, Key::Enter, Modifiers::empty());
        block_on(settle(&mut dom));
        assert!(
            !dioxus_ssr::render(&dom).contains("2027"),
            "with no note there is nothing to substitute in"
        );
    }

    #[test]
    fn visual_p_with_no_clipboard_seam_at_all_changes_nothing() {
        let vault = temp_vault();
        // rendered_app installs no Clipboard context
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);
        for key in ["v", "l", "p"] {
            press(
                &mut dom,
                sink,
                Key::Character(key.into()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before);
    }

    #[test]
    fn visual_p_over_a_failed_or_empty_read_leaves_the_selection_standing() {
        let vault = temp_vault();
        let (mut dom, clicks, _) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Err("read denied".to_string()),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);
        for key in ["v", "l", "p"] {
            press(
                &mut dom,
                sink,
                Key::Character(key.into()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before, "the text survived the refusal");
        assert!(
            dioxus_ssr::render(&dom).contains("clipboard: read denied"),
            "and the failed read did not stay silent (AIR ERR-2)"
        );

        // an empty clipboard is not a refusal, and still replaces nothing
        let (mut dom, clicks, _) =
            clipboard_app(Some(vault.path().to_path_buf()), Ok(String::new()));
        let (_, sink) = activate_heading(&mut dom, &clicks);
        let before = source_of(&dom);
        for key in ["v", "l", "p"] {
            press(
                &mut dom,
                sink,
                Key::Character(key.into()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));
        assert_eq!(source_of(&dom), before);
    }

    #[test]
    fn a_line_wise_visual_p_keeps_the_clips_own_newline() {
        let vault = temp_vault();
        let (mut dom, clicks, _) = clipboard_app(
            Some(vault.path().to_path_buf()),
            Ok("= collée\n".to_string()),
        );
        let (_, sink) = activate_heading(&mut dom, &clicks);
        for key in ["V", "p"] {
            press(
                &mut dom,
                sink,
                Key::Character(key.into()),
                Modifiers::empty(),
            );
        }
        block_on(settle(&mut dom));
        assert!(
            dioxus_ssr::render(&dom).contains("collée"),
            "the whole line went over"
        );
    }
}
