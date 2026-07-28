//! Time-note ids derived from a date: day, ISO week, season.
//!
//! Derivation is total — every date has all three ids whether or not the
//! notes exist; existence is the index's answer, checked at call sites
//! (`adr/2026-07-time-navigation-derived-not-stored.md`).

use jiff::civil::Date;

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

/// "2026-summer" — the only function that knows season boundaries.
/// Seasons are school semesters (`adr/2026-07-seasons-school-semesters.md`):
/// three per year, none crossing New Year, so the prefix is always the
/// calendar year.
pub fn season_id(date: Date) -> String {
    let season = match date.month() {
        1..=4 => "winter",
        5..=8 => "summer",
        _ => "autumn", // 9..=12
    };
    format!("{}-{season}", date.year())
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
}
