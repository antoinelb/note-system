//! Pure logic behind the logs screen: rail ordering, month grid,
//! breadcrumbs, labels. Everything decidable without a VirtualDom lives
//! here, so the components stay wiring (`adr/2026-07-ui-covered-at-100.md`).

use jiff::ToSpan;
use jiff::civil::Date;

use crate::domain::NoteType;
use crate::time;

/// What the user is looking at: one of the three time scales plus its id.
pub type Selection = (NoteType, String);

/// What an unsummarized capture is tagged with, wherever it is listed — the
/// "captured today" block and the open-loops list say the same thing.
pub const STILL_OPEN: &str = "still open";

/// One rail line. `exists: false` marks the spliced-in selected id whose
/// note is not on disk — rendered dim + italic, never written by navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailRow {
    pub id: String,
    pub scale: NoteType,
    pub exists: bool,
}

/// The rail: every time note ordered newest first, indentation carrying the
/// scale — season ⊃ week ⊃ day (`adr/2026-07-rail-continuous-newest-first.md`).
/// A selected id with no note is spliced into its chronological slot; its
/// missing ancestors are not synthesized, matching how an existing orphan
/// (a week with no season note) already renders.
pub fn rail_rows(
    notes: &[(String, NoteType)],
    selected: Option<&Selection>,
) -> Vec<RailRow> {
    let mut rows = Vec::new();
    for (id, scale) in notes {
        // a time note whose id does not parse as its scale cannot be placed
        // on a time axis — it stays reachable as a file, just not as a row
        if let Some(key) = sort_key(scale, id) {
            rows.push((
                key,
                RailRow {
                    id: id.clone(),
                    scale: scale.clone(),
                    exists: true,
                },
            ));
        }
    }
    if let Some((scale, id)) = selected
        && !notes.iter().any(|(existing, _)| existing == id)
        && let Some(key) = sort_key(scale, id)
    {
        rows.push((
            key,
            RailRow {
                id: id.clone(),
                scale: scale.clone(),
                exists: false,
            },
        ));
    }
    rows.sort_by_key(|(key, _)| std::cmp::Reverse(*key)); // newest first
    rows.into_iter().map(|(_, row)| row).collect()
}

/// The hierarchical sort key: `(season start, Monday, day)` descending,
/// with `Date::MAX` as the header sentinel so a scale's row sorts above its
/// children. A week belongs to the season of its Monday, and a day stays
/// inside its week's block (`adr/2026-07-time-note-period-conventions.md`).
fn sort_key(scale: &NoteType, id: &str) -> Option<(Date, Date, Date)> {
    match scale {
        NoteType::Daily => time::parse_day(id).map(|day| {
            let monday = time::monday_of(day);
            (time::season_start(monday), monday, day)
        }),
        NoteType::Weekly => time::parse_week(id)
            .map(|monday| (time::season_start(monday), monday, Date::MAX)),
        NoteType::Seasonal => {
            time::parse_season(id).map(|start| (start, Date::MAX, Date::MAX))
        }
        _ => None,
    }
}

/// The rail's right-aligned kind tag; day rows carry none — the date is
/// the row.
pub fn rail_tag(scale: &NoteType) -> &'static str {
    match scale {
        NoteType::Seasonal => "season",
        NoteType::Weekly => "week",
        _ => "",
    }
}

/// One month-grid line: the clickable ISO week id, its gutter label (the
/// zero-padded week number alone), and seven cells, `None` where the
/// column falls outside the month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridWeek {
    pub week_id: String,
    pub label: String,
    pub days: [Option<Date>; 7],
}

/// The jump panel's month: Monday-aligned weeks covering the month of
/// `month` (any day of it).
pub fn month_grid(month: Date) -> Vec<GridWeek> {
    let first = month.first_of_month();
    let last = month.last_of_month();
    let mut monday = time::monday_of(first);
    let mut weeks = Vec::new();
    // bounded: a month spans at most six Monday-aligned weeks
    for _ in 0..6 {
        if monday > last {
            break;
        }
        let days = std::array::from_fn(|offset| {
            let day = monday.saturating_add((offset as i64).days());
            (day.month() == first.month() && day.year() == first.year())
                .then_some(day)
        });
        weeks.push(GridWeek {
            week_id: time::week_id(monday),
            label: format!("{:02}", monday.iso_week_date().week()),
            days,
        });
        monday = monday.saturating_add(7.days());
    }
    weeks
}

/// One month forward or back, normalized to the first of the month.
/// Saturating at jiff's year bounds, which no amount of scrolling reaches
/// in a human lifetime of Wednesdays.
pub fn page_month(month: Date, forward: bool) -> Date {
    let step = if forward { 1 } else { -1 };
    month.first_of_month().saturating_add(step.months())
}

/// "july 2026" — the month header; the stylesheet uppercases it.
pub fn month_label(month: Date) -> String {
    month.strftime("%B %Y").to_string().to_lowercase()
}

/// The three seasons of the displayed year as `(label, selection)`, oldest
/// first — the row under the grid.
pub fn season_row(month: Date) -> [(&'static str, Selection); 3] {
    let year = month.year();
    [
        ("winter", (NoteType::Seasonal, format!("{year}-winter"))),
        ("summer", (NoteType::Seasonal, format!("{year}-summer"))),
        ("autumn", (NoteType::Seasonal, format!("{year}-autumn"))),
    ]
}

/// One breadcrumb segment above the centre pane; `target` is `Some` where
/// the segment jumps to a coarser scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub target: Option<Selection>,
}

/// "2026-07-23 · daily · w30 · summer 2026" — the id and its scale first,
/// then the coarser scales as jumps, derived from the date by jiff, never
/// stored (`adr/2026-07-time-navigation-derived-not-stored.md`).
pub fn breadcrumbs(scale: &NoteType, id: &str) -> Vec<Crumb> {
    let Some(date) = selection_date(scale, id) else {
        return Vec::new();
    };
    let mut crumbs = vec![
        Crumb {
            label: id.to_string(),
            target: None,
        },
        Crumb {
            label: scale.as_name().to_string(),
            target: None,
        },
    ];
    if *scale == NoteType::Daily {
        crumbs.push(Crumb {
            label: week_label(date),
            target: Some((NoteType::Weekly, time::week_id(date))),
        });
    }
    if *scale != NoteType::Seasonal {
        crumbs.push(Crumb {
            label: season_label(date),
            target: Some((NoteType::Seasonal, time::season_id(date))),
        });
    }
    crumbs
}

/// The date a selection lives on: the day itself, the week's Monday, the
/// season's first day. `None` only for ids no formatter of ours produced.
pub fn selection_date(scale: &NoteType, id: &str) -> Option<Date> {
    match scale {
        NoteType::Daily => time::parse_day(id),
        NoteType::Weekly => time::parse_week(id),
        NoteType::Seasonal => time::parse_season(id),
        _ => None,
    }
}

/// The human name of a selection for the empty-note line: "july 24",
/// "w30", "summer 2026"; an unplaceable id falls back to itself.
pub fn selection_label(scale: &NoteType, id: &str) -> String {
    match (selection_date(scale, id), scale) {
        (Some(date), NoteType::Daily) => {
            format!("{} {}", month_name(date), date.day())
        }
        (Some(date), NoteType::Weekly) => week_label(date),
        (Some(date), _) => season_label(date),
        (None, _) => id.to_string(),
    }
}

/// "capture-articles-zettel · still open" — one line of the "captured today"
/// block. A capture still owing its summary says so instead of naming its
/// category, which the day already knows it by
/// (adr/2026-08-summarized-nonempty-summary-section.md); everything else is
/// tagged by category.
pub fn captured_line(
    stem: &str,
    category: &crate::domain::NoteCategory,
    still_open: bool,
) -> String {
    let tag = if still_open {
        STILL_OPEN
    } else {
        category.as_dir()
    };
    format!("{stem} · {tag}")
}

/// "w30" — the short week form used in crumbs and empty-note lines.
fn week_label(date: Date) -> String {
    format!("w{:02}", date.iso_week_date().week())
}

/// "summer 2026" — the season id's parts, swapped for prose.
fn season_label(date: Date) -> String {
    format!("{} {}", time::season_name(date), date.year())
}

/// "july" — lowercase like every UI string on the screen.
fn month_name(date: Date) -> String {
    date.strftime("%B").to_string().to_lowercase()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::domain::NoteCategory;

    use super::*;

    fn date(iso: &str) -> Date {
        iso.parse().expect("test dates are valid ISO dates")
    }

    fn daily(id: &str) -> (String, NoteType) {
        (id.to_string(), NoteType::Daily)
    }

    fn weekly(id: &str) -> (String, NoteType) {
        (id.to_string(), NoteType::Weekly)
    }

    fn seasonal(id: &str) -> (String, NoteType) {
        (id.to_string(), NoteType::Seasonal)
    }

    fn ids(rows: &[RailRow]) -> Vec<&str> {
        rows.iter().map(|row| row.id.as_str()).collect()
    }

    // -- the rail: one list, indentation carries the scale -------------------

    #[test]
    fn rail_orders_newest_first_with_headers_above_their_children() {
        let rows = rail_rows(
            &[
                daily("2026-01-15"),
                daily("2026-07-22"),
                daily("2026-07-23"),
                daily("2026-07-30"),
                weekly("2026-w30"),
                weekly("2026-w31"),
                seasonal("2026-summer"),
                seasonal("2026-winter"),
            ],
            None,
        );
        assert_eq!(
            ids(&rows),
            [
                "2026-summer",
                "2026-w31",
                "2026-07-30",
                "2026-w30",
                "2026-07-23",
                "2026-07-22",
                "2026-winter",
                "2026-01-15",
            ]
        );
        assert!(rows.iter().all(|row| row.exists));
    }

    #[test]
    fn a_day_sorts_under_its_mondays_season_not_its_own() {
        // Friday 2026-05-01 is summer's first day, but its Monday
        // (2026-04-27) is winter's — the day stays inside its week block
        // (adr/2026-07-time-note-period-conventions.md)
        let rows = rail_rows(
            &[
                daily("2026-05-01"),
                seasonal("2026-summer"),
                seasonal("2026-winter"),
            ],
            None,
        );
        assert_eq!(ids(&rows), ["2026-summer", "2026-winter", "2026-05-01"]);
    }

    #[test]
    fn a_new_year_day_sorts_under_the_old_years_autumn() {
        let rows = rail_rows(
            &[
                daily("2027-01-01"),
                weekly("2026-w53"),
                seasonal("2026-autumn"),
                seasonal("2027-winter"),
            ],
            None,
        );
        assert_eq!(
            ids(&rows),
            ["2027-winter", "2026-autumn", "2026-w53", "2027-01-01"]
        );
    }

    #[test]
    fn a_selected_missing_day_is_spliced_into_its_slot() {
        // no w31 note exists either: the spliced day renders as an orphan
        // above the w30 block, no ancestors synthesized
        let selected = (NoteType::Daily, "2026-07-30".to_string());
        let rows = rail_rows(
            &[daily("2026-07-23"), weekly("2026-w30")],
            Some(&selected),
        );
        assert_eq!(ids(&rows), ["2026-07-30", "2026-w30", "2026-07-23"]);
        assert!(!rows[0].exists);
        assert!(rows[1].exists && rows[2].exists);
    }

    #[test]
    fn a_selected_existing_note_is_not_duplicated() {
        let selected = (NoteType::Daily, "2026-07-23".to_string());
        let rows = rail_rows(&[daily("2026-07-23")], Some(&selected));
        assert_eq!(ids(&rows), ["2026-07-23"]);
        assert!(rows[0].exists);
    }

    #[test]
    fn unplaceable_rows_and_selections_are_left_out() {
        let selected = (NoteType::Concept, "not-time".to_string());
        let rows = rail_rows(
            &[
                daily("garbage"),
                weekly("2026-07-23"),
                seasonal("2026-spring"),
                ("essay".to_string(), NoteType::Concept),
                daily("2026-07-23"),
            ],
            Some(&selected),
        );
        assert_eq!(ids(&rows), ["2026-07-23"]);
    }

    #[test]
    fn rail_tags_name_the_wider_scales_only() {
        assert_eq!(rail_tag(&NoteType::Seasonal), "season");
        assert_eq!(rail_tag(&NoteType::Weekly), "week");
        assert_eq!(rail_tag(&NoteType::Daily), "");
    }

    // -- the month grid ------------------------------------------------------

    #[test]
    fn july_2026_spans_five_monday_aligned_weeks() {
        let weeks = month_grid(date("2026-07-23"));
        let week_ids: Vec<&str> =
            weeks.iter().map(|week| week.week_id.as_str()).collect();
        assert_eq!(
            week_ids,
            ["2026-w27", "2026-w28", "2026-w29", "2026-w30", "2026-w31"]
        );
        let labels: Vec<&str> =
            weeks.iter().map(|week| week.label.as_str()).collect();
        assert_eq!(labels, ["27", "28", "29", "30", "31"]);
        // July 1st is a Wednesday: two leading blanks
        assert_eq!(weeks[0].days[0], None);
        assert_eq!(weeks[0].days[1], None);
        assert_eq!(weeks[0].days[2], Some(date("2026-07-01")));
        // the last row ends Friday the 31st, two trailing blanks
        assert_eq!(weeks[4].days[4], Some(date("2026-07-31")));
        assert_eq!(weeks[4].days[5], None);
        assert_eq!(weeks[4].days[6], None);
    }

    #[test]
    fn august_2026_needs_all_six_rows() {
        // August 1st is a Saturday: 5 leading blanks + 31 days = 36 cells
        assert_eq!(month_grid(date("2026-08-15")).len(), 6);
    }

    #[test]
    fn february_2021_is_exactly_four_full_weeks() {
        let weeks = month_grid(date("2021-02-14"));
        assert_eq!(weeks.len(), 4);
        assert!(
            weeks
                .iter()
                .all(|week| week.days.iter().all(Option::is_some)),
            "no blanks in a month that starts on Monday"
        );
    }

    #[test]
    fn paging_normalizes_to_the_first_and_crosses_years() {
        assert_eq!(page_month(date("2026-07-23"), true), date("2026-08-01"));
        assert_eq!(page_month(date("2026-01-15"), false), date("2025-12-01"));
        assert_eq!(page_month(date("2026-12-31"), true), date("2027-01-01"));
    }

    // -- labels and crumbs ---------------------------------------------------

    #[test]
    fn month_label_is_lowercase_prose() {
        assert_eq!(month_label(date("2026-07-23")), "july 2026");
    }

    #[test]
    fn season_row_lists_the_displayed_years_seasons() {
        let [winter, summer, autumn] = season_row(date("2026-07-23"));
        assert_eq!(winter.0, "winter");
        assert_eq!(winter.1, (NoteType::Seasonal, "2026-winter".to_string()));
        assert_eq!(summer.1.1, "2026-summer");
        assert_eq!(autumn.1.1, "2026-autumn");
    }

    #[test]
    fn daily_breadcrumbs_chain_all_three_scales() {
        let crumbs = breadcrumbs(&NoteType::Daily, "2026-07-23");
        let labels: Vec<&str> =
            crumbs.iter().map(|crumb| crumb.label.as_str()).collect();
        assert_eq!(labels, ["2026-07-23", "daily", "w30", "summer 2026"]);
        assert_eq!(crumbs[0].target, None);
        assert_eq!(crumbs[1].target, None);
        assert_eq!(
            crumbs[2].target,
            Some((NoteType::Weekly, "2026-w30".to_string()))
        );
        assert_eq!(
            crumbs[3].target,
            Some((NoteType::Seasonal, "2026-summer".to_string()))
        );
    }

    #[test]
    fn weekly_breadcrumbs_jump_to_the_mondays_season() {
        let crumbs = breadcrumbs(&NoteType::Weekly, "2026-w30");
        let labels: Vec<&str> =
            crumbs.iter().map(|crumb| crumb.label.as_str()).collect();
        assert_eq!(labels, ["2026-w30", "weekly", "summer 2026"]);
        assert_eq!(
            crumbs[2].target,
            Some((NoteType::Seasonal, "2026-summer".to_string()))
        );
    }

    #[test]
    fn seasonal_breadcrumbs_have_nowhere_wider_to_jump() {
        let crumbs = breadcrumbs(&NoteType::Seasonal, "2026-summer");
        let labels: Vec<&str> =
            crumbs.iter().map(|crumb| crumb.label.as_str()).collect();
        assert_eq!(labels, ["2026-summer", "seasonal"]);
    }

    #[test]
    fn an_unplaceable_selection_gets_no_crumbs() {
        assert_eq!(breadcrumbs(&NoteType::Daily, "garbage"), Vec::new());
        assert_eq!(breadcrumbs(&NoteType::Concept, "essay"), Vec::new());
    }

    #[test]
    fn selection_labels_speak_each_scale() {
        assert_eq!(selection_label(&NoteType::Daily, "2026-07-24"), "july 24");
        assert_eq!(selection_label(&NoteType::Weekly, "2026-w30"), "w30");
        assert_eq!(
            selection_label(&NoteType::Seasonal, "2026-summer"),
            "summer 2026"
        );
        assert_eq!(selection_label(&NoteType::Daily, "garbage"), "garbage");
    }

    #[test]
    fn captured_lines_tag_the_category_or_the_open_loop() {
        assert_eq!(
            captured_line(
                "capture-idea-canvas",
                &NoteCategory::Capture,
                false
            ),
            "capture-idea-canvas · capture"
        );
        assert_eq!(
            captured_line(
                "capture-articles-zettel",
                &NoteCategory::Capture,
                true
            ),
            "capture-articles-zettel · still open"
        );
        assert_eq!(
            captured_line(
                "digest-smart-notes",
                &NoteCategory::Generated,
                false
            ),
            "digest-smart-notes · generated"
        );
    }
}
