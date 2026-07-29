use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use dioxus::prelude::*;
use jiff::civil::Date;

use crate::domain::{NoteCategory, NoteType};
use crate::index::{Index, IndexError};
use crate::logs::{self, Selection};
use crate::render::SvgCache;
use crate::time;

#[derive(Clone, Debug)]
pub struct VaultRoot(pub Option<PathBuf>);

/// How Ctrl+Q reaches the windowing system: `main` injects the real
/// window-close call, the headless tests inject a recorder — the same
/// root-context channel as `VaultRoot`. With the centre pane read-only
/// there is nothing to flush first (adr/2026-07-logs-centre-read-only.md).
#[derive(Clone)]
pub struct Closer(pub Arc<dyn Fn() + Send + Sync>);

/// Today's date, injected at the root by `main` — the app's single clock
/// edge, replaced by a fixed date in the headless tests
/// (adr/2026-07-today-injected-root-context.md).
#[derive(Clone, Copy, Debug)]
pub struct Today(pub Date);

#[component]
pub fn App() -> Element {
    let vault = use_context::<VaultRoot>();
    let today = use_context::<Today>();
    let loaded = use_hook(|| load(vault.0));
    let mut light = use_signal(|| false);
    // absent in the headless tests, where there is no window to close
    let closer = try_consume_context::<Closer>();
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
                    light.set(!light());
                } else if event.modifiers().ctrl()
                    && event.key() == Key::Character("q".to_string())
                    && let Some(closer) = &closer
                {
                    (closer.0)();
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
    loops: usize,
    today: Date,
) -> Element {
    let mut notes = use_signal(|| notes);
    let mut selected = use_signal(|| (NoteType::Daily, time::day_id(today)));
    let mut month = use_signal(|| today.first_of_month());
    let mut notice = use_signal(|| None::<String>);
    // the SVG cache is a memo store, not UI state: nothing should re-render
    // when it fills, so a plain hook value rather than a signal
    let cache = use_hook(|| Rc::new(RefCell::new(SvgCache::default())));

    let select = use_callback(move |target: Selection| {
        let anchor = logs::selection_date(&target.0, &target.1);
        // every selectable id comes from our own formatters, so the today
        // fallback guards the type system, not a reachable path
        month.set(anchor.unwrap_or(today).first_of_month());
        selected.set(target);
        notice.set(None);
    });

    let (scale, id) = selected();
    let note_list = notes();
    let exists = note_list.iter().any(|(existing, _)| existing == &id);
    let rows = logs::rail_rows(&note_list, Some(&(scale.clone(), id.clone())));
    let crumbs = logs::breadcrumbs(&scale, &id);
    let rendered =
        exists.then(|| render_note(&root, &id, &mut cache.borrow_mut()));
    let captured = (exists && scale == NoteType::Daily)
        .then(|| captured_lines(&root, &id));
    let day_ids: HashSet<&str> =
        note_list.iter().map(|(note, _)| note.as_str()).collect();
    let weeks = logs::month_grid(month());
    let seasons = logs::season_row(month());

    let keyboard = {
        let root = root.clone();
        move |event: KeyboardEvent| {
            match event.key() {
                // months page by keystroke as well as by scrolling; arrows
                // move the grid only, never the selection
                // (adr/2026-07-month-paging-arrow-keys.md)
                Key::ArrowLeft => {
                    month.set(logs::page_month(month(), false));
                }
                Key::ArrowRight => {
                    month.set(logs::page_month(month(), true));
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
                        Ok(_) => notes.with_mut(|list| list.push((id, scale))),
                        Err(err) => {
                            notice.set(Some(format!("create: {err:?}")))
                        }
                    }
                }
                _ => {}
            }
        }
    };

    rsx! {
        Chrome { screen: Screen::Logs, loops }
        div {
            class: "logs",
            // the enter-to-create keystroke lands here and the theme/quit
            // chords bubble on up to the .app root
            tabindex: "0",
            autofocus: true,
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
                    match notice() {
                        Some(msg) => rsx! { p { class: "render-error", "{msg}" } },
                        None => rsx! {},
                    }
                }
                {
                    match &rendered {
                        Some(Ok(svg)) => rsx! {
                            div { class: "note", dangerous_inner_html: "{svg}" }
                        },
                        Some(Err(msg)) => rsx! {
                            p { class: "render-error", "{msg}" }
                        },
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
                        month.set(logs::page_month(month(), delta > 0.0));
                    }
                },
                div { class: "cal-head",
                    span { class: "cal-month type-label", "{logs::month_label(month())}" }
                    // ‹ today › — the mockup's header controls
                    // (adr/2026-07-month-paging-arrow-keys.md)
                    span { class: "cal-nav",
                        button {
                            class: "cal-arrow",
                            onclick: move |_| {
                                month.set(logs::page_month(month(), false))
                            },
                            "‹"
                        }
                        button {
                            class: "cal-today",
                            onclick: move |_| {
                                select.call((NoteType::Daily, time::day_id(today)))
                            },
                            "today"
                        }
                        button {
                            class: "cal-arrow",
                            onclick: move |_| {
                                month.set(logs::page_month(month(), true))
                            },
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

/// What the shell mounts with: the vault root, the rail's time notes and
/// the ember count.
type Loaded = (PathBuf, Vec<(String, NoteType)>, usize);

fn load(root: Option<PathBuf>) -> Result<Loaded, String> {
    match root {
        Some(root) => match load_notes(&root) {
            Ok((notes, loops)) => Ok((root, notes, loops)),
            Err(err) => Err(format!("the index could not be built: {err:?}")),
        },
        None => Err("no vault: define NOTE_VAULT or HOME".to_string()),
    }
}

fn load_notes(
    root: &Path,
) -> Result<(Vec<(String, NoteType)>, usize), IndexError> {
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
fn survey(
    index: &Index,
) -> Result<(Vec<(String, NoteType)>, usize), IndexError> {
    Ok((index.time_notes()?, loop_count(index)?))
}

/// The v0 open-loops count: typeless notes + dangling links
/// (adr/2026-07-debt-counter-then-list.md). Unsummarized captures join in
/// phase 10, which also moves the count onto the watcher.
fn loop_count(index: &Index) -> Result<usize, IndexError> {
    Ok(index.typeless_notes()?.len() + index.dangling_links()?.len())
}

/// The centre pane's note: a time note's id is its stem, so the path needs
/// no index round-trip. Read from disk, compiled through the cache.
fn render_note(
    root: &Path,
    id: &str,
    cache: &mut SvgCache,
) -> Result<String, String> {
    let file = root
        .join(NoteCategory::Time.as_dir())
        .join(format!("{id}.typ"));
    let text = std::fs::read_to_string(&file)
        .map_err(|err| format!("{}: {err}", file.display()))?;
    cache
        .render(root, &file, &text)
        .map_err(|err| format!("{err:?}"))
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
    Ok(captured
        .iter()
        .map(|(stem, category)| logs::captured_line(stem, category))
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
    /// day cells — then the two crumb jumps, then the five rail rows top to
    /// bottom.
    const CAL_BACK: usize = 0;
    const CAL_TODAY: usize = 1;
    const CAL_FORWARD: usize = 2;
    const SEASON_AUTUMN: usize = 5;
    const GUTTER_W31: usize = 36;
    const CRUMB_WEEK: usize = 42;
    const RAIL_SUMMER: usize = 44;
    const RAIL_W30: usize = 45;
    const RAIL_DAY_23: usize = 46;
    const RAIL_DAY_22: usize = 47;
    const RAIL_DAY_21: usize = 48;
    /// July 2026 leads with two blanks, so a date's cell index is offset by
    /// one gutter per started week row.
    const fn day_cell(day: usize) -> usize {
        6 + (day + 1) / 7 + day
    }
    /// Which keydown listener is the logs pane's (the other is the root).
    const LOGS_KEYS: usize = 1;

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
        let (mut dom, keydown, closed) =
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
    fn ctrl_q_on_the_vault_error_screen_closes_immediately() {
        let (mut dom, keydown, closed) = quit_app(None);
        press(
            &mut dom,
            keydown,
            Key::Character("q".into()),
            Modifiers::CONTROL,
        );
        assert!(closed.load(Ordering::SeqCst));
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
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
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
        let (dom, _, _, _) = rendered_app(Some(vault.path().to_path_buf()));
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains(r#"<span class="ember">1</span>"#), "{html}");
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
            49,
            "3 header + 3 seasons + 5 gutters + 31 days + 2 crumbs + 5 rail: {html}"
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
    fn a_note_that_cannot_compile_shows_the_render_error() {
        let vault = temp_vault();
        let (mut dom, clicks, _, _) =
            rendered_app(Some(vault.path().to_path_buf()));
        click(&mut dom, clicks[RAIL_DAY_21]);
        let html = dioxus_ssr::render(&dom);
        assert!(html.contains("render-error"), "{html}");
        assert!(!html.contains(RENDERED_NOTE), "{html}");
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
        let error = loop_count(&index).unwrap_err();
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
    /// harness for the quit chord — returning the app-root keydown target
    /// and the flag the closer sets.
    fn quit_app(
        root: Option<PathBuf>,
    ) -> (VirtualDom, ElementId, Arc<std::sync::atomic::AtomicBool>) {
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let recorder = closed.clone();
        let closer = Closer(Arc::new(move || {
            recorder.store(true, Ordering::SeqCst);
        }));
        let (dom, mutations) = mounted_app(root, Some(closer));
        let keydown = listeners(&mutations, "keydown")[0];
        (dom, keydown, closed)
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
        set_event_converter(Box::new(TestEvents));
        let mut dom = VirtualDom::new(App);
        dom.insert_any_root_context(Box::new(VaultRoot(root)));
        dom.insert_any_root_context(Box::new(Today(
            TODAY.parse().expect("the test clock is a valid date"),
        )));
        if let Some(closer) = closer {
            dom.insert_any_root_context(Box::new(closer));
        }
        let mutations = dom.rebuild_to_vec();
        (dom, mutations)
    }

    /// Fires a keydown. The physical code is irrelevant to every handler,
    /// which read only the key and its modifiers.
    fn press(
        dom: &mut VirtualDom,
        target: ElementId,
        key: Key,
        modifiers: Modifiers,
    ) {
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
        dom.runtime()
            .handle_event("keydown", Event::new(data, true), target);
        dom.process_events();
        dom.render_immediate_to_vec();
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

    /// Fires a wheel event with the given vertical pixel delta.
    fn scroll(dom: &mut VirtualDom, target: ElementId, delta_y: f64) {
        let data: Rc<dyn Any> =
            Rc::new(PlatformEventData::new(Box::new(SerializedWheelData {
                mouse: SerializedPointInteraction::default(),
                delta_mode: 0, // pixels
                delta_x: 0.0,
                delta_y,
                delta_z: 0.0,
            })));
        dom.runtime()
            .handle_event("wheel", Event::new(data, true), target);
        dom.process_events();
        dom.render_immediate_to_vec();
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
            ("time/2026-07-22.typ", time_note("2026-07-22", "daily")),
            ("time/2026-07-23.typ", time_note("2026-07-23", "daily")),
            ("time/2026-w30.typ", time_note("2026-w30", "weekly")),
            ("time/2026-summer.typ", time_note("2026-summer", "seasonal")),
            (
                "capture/capture-idea.typ",
                format!(
                    "#import \"/templates/template.typ\": *\n\
                     #show: note\n\
                     #meta(id: \"capture-idea\", created: \"{TODAY}\")\n\
                     \n= capture-idea\n"
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

    fn note(id: &str) -> String {
        format!(
            "#import \"/templates/template.typ\": *\n\
             #show: note\n\
             #meta(id: \"{id}\", type: \"concept\", created: \"2026-07-01\")\n\
             \n= {id}\n"
        )
    }

    /// Only mouse, keyboard and wheel events are real: the shell listens for
    /// clicks everywhere, keydowns on the two roots and wheel on the jump
    /// panel, so every other conversion is unreachable in these tests.
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

        fn convert_form_data(&self, _: &PlatformEventData) -> FormData {
            unreachable!("the read-only shell mounts no inputs")
        }

        fn convert_image_data(&self, _: &PlatformEventData) -> ImageData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_media_data(&self, _: &PlatformEventData) -> MediaData {
            unreachable!("the shell never listens for this event")
        }

        fn convert_mounted_data(&self, _: &PlatformEventData) -> MountedData {
            unreachable!("the shell never listens for this event")
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
