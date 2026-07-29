//! Time-note ids derived from a date: day, ISO week, season.
//!
//! Derivation is total — every date has all three ids whether or not the
//! notes exist; existence is the index's answer, checked at call sites
//! (`adr/2026-07-time-navigation-derived-not-stored.md`).

use jiff::ToSpan;
use jiff::civil::{Date, ISOWeekDate, Weekday};

/// The one clock read in the app: `main` injects this value at the root,
/// everything below it takes the date as a parameter
/// (adr/2026-07-today-injected-root-context.md).
pub fn today() -> Date {
    jiff::Zoned::now().date()
}

/// The ids a date belongs to, smallest scale first: day, week, season.
pub fn scale_chain(date: Date) -> [String; 3] {
    [day_id(date), week_id(date), season_id(date)]
}

/// "2026-07-23" — the day id is the ISO date itself.
pub fn day_id(date: Date) -> String {
    date.to_string()
}

/// "2026-w30" — ISO week-year prefix, zero-padded week number, so
/// lexicographic order is chronological (`adr/2026-07-seasons-school-semesters.md`).
/// Around New Year the prefix can differ from the calendar year.
pub fn week_id(date: Date) -> String {
    let week = date.iso_week_date();
    format!("{}-w{:02}", week.year(), week.week())
}

/// "2026-summer" — seasons are school semesters
/// (`adr/2026-07-seasons-school-semesters.md`): three per year, none
/// crossing New Year, so the prefix is always the calendar year.
pub fn season_id(date: Date) -> String {
    format!("{}-{}", date.year(), season_name(date))
}

/// The month → season-name mapping; `season_start` and `parse_season` hold
/// its mirrors, and the boundary tests pin all three together.
pub fn season_name(date: Date) -> &'static str {
    match date.month() {
        1..=4 => "winter",
        5..=8 => "summer",
        _ => "autumn", // 9..=12
    }
}

/// The Monday of the week holding `date`. Saturating: the subtraction can
/// only clamp at jiff's year −9999 floor, which no real vault reaches.
pub fn monday_of(date: Date) -> Date {
    let offset = i64::from(date.weekday().to_monday_zero_offset());
    date.saturating_sub(offset.days())
}

/// The first day of the season holding `date` — the rail's grouping key
/// and a seasonal note's `created`
/// (adr/2026-07-time-note-period-conventions.md).
pub fn season_start(date: Date) -> Date {
    let month = match date.month() {
        1..=4 => 1,
        5..=8 => 5,
        _ => 9,
    };
    // same year, day 1: cannot fail, so the fallback is never a lie
    Date::new(date.year(), month, 1).unwrap_or(date)
}

/// "2026-07-23" back to its date. All three parsers answer `None` for a
/// malformed id — selection ids come from our own formatters, but totality
/// is cheap and keeps callers branch-free.
pub fn parse_day(id: &str) -> Option<Date> {
    id.parse().ok()
}

/// "2026-w30" back to its Monday, the weekly note's `created`.
pub fn parse_week(id: &str) -> Option<Date> {
    let (year, week) = id.split_once("-w")?;
    let week = ISOWeekDate::new(
        year.parse().ok()?,
        week.parse().ok()?,
        Weekday::Monday,
    )
    .ok()?;
    Some(week.date())
}

/// "2026-summer" back to its first day, the seasonal note's `created`.
pub fn parse_season(id: &str) -> Option<Date> {
    let (year, season) = id.split_once('-')?;
    let month = match season {
        "winter" => 1,
        "summer" => 5,
        "autumn" => 9,
        _ => return None,
    };
    Date::new(year.parse().ok()?, month, 1).ok()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn date(iso: &str) -> Date {
        iso.parse().expect("test dates are valid ISO dates")
    }

    #[test]
    fn day_id_is_the_iso_date() {
        assert_eq!(day_id(date("2026-07-23")), "2026-07-23");
    }

    #[test]
    fn week_id_matches_the_fixture_week() {
        // time/2026-w30.typ was created on Monday 2026-07-20
        assert_eq!(week_id(date("2026-07-23")), "2026-w30");
    }

    #[test]
    fn single_digit_weeks_are_zero_padded() {
        // 2026-01-05 is the Monday of ISO week 2
        assert_eq!(week_id(date("2026-01-05")), "2026-w02");
    }

    #[test]
    fn early_january_can_belong_to_the_previous_iso_year() {
        // 2026-01-01 is a Thursday, so ISO year 2026 has 53 weeks and
        // Friday 2027-01-01 still falls in its last one
        assert_eq!(week_id(date("2027-01-01")), "2026-w53");
    }

    #[test]
    fn late_december_can_belong_to_the_next_iso_year() {
        // Monday 2024-12-30 opens the week holding 2025's first Thursday
        assert_eq!(week_id(date("2024-12-30")), "2025-w01");
    }

    #[test]
    fn season_edges_fall_on_semester_boundaries() {
        assert_eq!(season_id(date("2026-04-30")), "2026-winter");
        assert_eq!(season_id(date("2026-05-01")), "2026-summer");
        assert_eq!(season_id(date("2026-08-31")), "2026-summer");
        assert_eq!(season_id(date("2026-09-01")), "2026-autumn");
    }

    #[test]
    fn season_and_year_flip_together_at_new_year() {
        assert_eq!(season_id(date("2026-12-31")), "2026-autumn");
        assert_eq!(season_id(date("2027-01-01")), "2027-winter");
    }

    #[test]
    fn the_chain_components_can_disagree_about_the_year() {
        assert_eq!(
            scale_chain(date("2027-01-01")),
            ["2027-01-01", "2026-w53", "2027-winter"]
        );
    }

    #[test]
    fn today_reads_the_clock_once() {
        // racing midnight is the only way this fails; acceptable odds
        assert_eq!(today(), jiff::Zoned::now().date());
    }

    #[test]
    fn monday_of_lands_on_the_week_start() {
        assert_eq!(monday_of(date("2026-07-23")), date("2026-07-20"));
        assert_eq!(monday_of(date("2026-07-20")), date("2026-07-20"));
        // New Year: Friday 2027-01-01 belongs to 2026's last ISO week
        assert_eq!(monday_of(date("2027-01-01")), date("2026-12-28"));
    }

    #[test]
    fn season_start_matches_the_semester_boundaries() {
        assert_eq!(season_start(date("2026-04-30")), date("2026-01-01"));
        assert_eq!(season_start(date("2026-05-01")), date("2026-05-01"));
        assert_eq!(season_start(date("2026-12-31")), date("2026-09-01"));
    }

    #[test]
    fn parse_day_inverts_day_id() {
        assert_eq!(parse_day("2026-07-23"), Some(date("2026-07-23")));
        assert_eq!(parse_day("2026-w30"), None);
        assert_eq!(parse_day("garbage"), None);
    }

    #[test]
    fn parse_week_yields_the_monday() {
        assert_eq!(parse_week("2026-w30"), Some(date("2026-07-20")));
        // 2026 has 53 ISO weeks; parsing does not require zero-padding
        assert_eq!(parse_week("2026-w53"), Some(date("2026-12-28")));
    }

    #[test]
    fn parse_week_rejects_every_malformed_shape() {
        assert_eq!(parse_week("2026-07-23"), None, "no -w marker");
        assert_eq!(parse_week("year-w30"), None, "unparsable year");
        assert_eq!(parse_week("2026-wxx"), None, "unparsable week");
        assert_eq!(parse_week("2025-w54"), None, "week out of range");
    }

    #[test]
    fn parse_season_yields_the_first_day() {
        assert_eq!(parse_season("2026-winter"), Some(date("2026-01-01")));
        assert_eq!(parse_season("2026-summer"), Some(date("2026-05-01")));
        assert_eq!(parse_season("2026-autumn"), Some(date("2026-09-01")));
    }

    #[test]
    fn parse_season_rejects_every_malformed_shape() {
        assert_eq!(parse_season("garbage"), None, "no separator");
        assert_eq!(parse_season("2026-spring"), None, "no such season");
        assert_eq!(parse_season("year-winter"), None, "unparsable year");
        assert_eq!(parse_season("30000-winter"), None, "year past jiff max");
    }
}
