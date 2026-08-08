//! Pure logic behind the table screen: the card each indexed note becomes,
//! the treatment its kind wears, and where an unplaced note stands until
//! phase 8 places it. Everything decidable without a VirtualDom lives here,
//! so the component stays wiring (adr/2026-07-ui-covered-at-100.md).

use jiff::civil::Date;

use crate::domain::{NoteCategory, NoteType};
use crate::index::TableNote;
use crate::positions::Positions;

/// Titles-zoom card width (wireframe 170–180, normalized to ×4).
pub const CARD_WIDTH: f64 = 176.0;

/// One card the canvas draws, position already resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: String,
    /// Falls back to the id — a card is never blank.
    pub title: String,
    /// The uppercase mono line over the title: the type name, or the
    /// capture's age ("capture · 3 d", plan.md § Friction system).
    pub label: String,
    pub kind: NoteCategory,
    /// The 3px bar's CSS class, one of a closed set — never minted from a
    /// type name, so an unknown type cannot invent a class no variable backs.
    pub bar: &'static str,
    pub x: f64,
    pub y: f64,
}

/// Resolve every table note against the store. Unplaced notes take the
/// deterministic fallback grid; input order (the query's ORDER BY id) is the
/// grid order, so the stack cannot shuffle between renders — dumb and honest
/// until phase 8 places them (roadmap-v1.md § Phase 2).
pub fn cards(
    notes: &[TableNote],
    positions: &Positions,
    today: Date,
) -> Vec<Card> {
    let mut unplaced = 0usize;
    notes
        .iter()
        .map(|note| {
            let (x, y) = match positions.get(&note.id) {
                Some(placed) => placed,
                None => {
                    let slot = fallback_slot(unplaced);
                    unplaced += 1;
                    slot
                }
            };
            Card {
                id: note.id.clone(),
                title: note.title.clone().unwrap_or_else(|| note.id.clone()),
                label: label(note, today),
                kind: note.kind,
                bar: bar_class(note),
                x,
                y,
            }
        })
        .collect()
}

const GRID_COLUMNS: usize = 4;
const GRID_ORIGIN: f64 = 32.0;
const GRID_GAP: f64 = 16.0;
const GRID_ROW_HEIGHT: f64 = 96.0;

/// The unplaced stack: a 4-wide grid at the canvas origin, visible where the
/// viewport starts. Rank counts only unplaced notes, so placing one by drag
/// closes the grid up on the next load rather than leaving holes.
fn fallback_slot(rank: usize) -> (f64, f64) {
    let column = (rank % GRID_COLUMNS) as f64;
    let row = (rank / GRID_COLUMNS) as f64;
    (
        GRID_ORIGIN + column * (CARD_WIDTH + GRID_GAP),
        GRID_ORIGIN + row * GRID_ROW_HEIGHT,
    )
}

/// The card's label line. The query never yields time notes, so the Time arm
/// only keeps the match total — it wears the permanent treatment rather than
/// a panic no test could reach.
fn label(note: &TableNote, today: Date) -> String {
    match note.kind {
        NoteCategory::Capture => {
            match age_days(note.created.as_deref(), today) {
                Some(days) => format!("capture · {days} d"),
                None => "capture".to_string(),
            }
        }
        NoteCategory::Generated => "generated".to_string(),
        NoteCategory::Permanent | NoteCategory::Time => {
            note.note_type.as_ref().map_or_else(
                || "untyped".to_string(),
                |t| t.as_name().to_string(),
            )
        }
    }
}

/// How many days old a capture is. An absent or unparseable date is no age
/// at all — the label degrades to a bare "capture", never an error; a
/// future-dated capture reads as today rather than a negative age.
fn age_days(created: Option<&str>, today: Date) -> Option<i32> {
    let date: Date = created?.parse().ok()?;
    Some((today - date).get_days().max(0))
}

/// The 3px bar: eight permanent hues; everything else — captures, unknown
/// or time-scale types, no type at all — is the grey of visible debt, and
/// generated notes keep their own dashed bar.
fn bar_class(note: &TableNote) -> &'static str {
    match note.kind {
        NoteCategory::Capture => "bar-untyped",
        NoteCategory::Generated => "bar-generated",
        NoteCategory::Permanent | NoteCategory::Time => {
            match &note.note_type {
                Some(NoteType::Person) => "bar-person",
                Some(NoteType::Organisation) => "bar-organisation",
                Some(NoteType::Source) => "bar-source",
                Some(NoteType::Concept) => "bar-concept",
                Some(NoteType::Claim) => "bar-claim",
                Some(NoteType::Idea) => "bar-idea",
                Some(NoteType::Personal) => "bar-personal",
                Some(NoteType::Project) => "bar-project",
                Some(_) | None => "bar-untyped",
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const TODAY: Date = Date::constant(2026, 7, 23);

    fn note(id: &str, kind: NoteCategory) -> TableNote {
        TableNote {
            id: id.to_string(),
            kind,
            note_type: None,
            title: None,
            created: None,
        }
    }

    fn typed(id: &str, note_type: NoteType) -> TableNote {
        TableNote {
            note_type: Some(note_type),
            ..note(id, NoteCategory::Permanent)
        }
    }

    fn empty_positions() -> Positions {
        Positions::load(std::path::Path::new("/nonexistent/positions"))
    }

    #[test]
    fn every_permanent_type_wears_its_own_bar() {
        let cases = [
            (NoteType::Person, "bar-person"),
            (NoteType::Organisation, "bar-organisation"),
            (NoteType::Source, "bar-source"),
            (NoteType::Concept, "bar-concept"),
            (NoteType::Claim, "bar-claim"),
            (NoteType::Idea, "bar-idea"),
            (NoteType::Personal, "bar-personal"),
            (NoteType::Project, "bar-project"),
        ];
        for (note_type, expected) in cases {
            let name = note_type.as_name().to_string();
            let drawn =
                cards(&[typed("a", note_type)], &empty_positions(), TODAY);
            assert_eq!(drawn[0].bar, expected, "for type {name}");
            assert_eq!(drawn[0].label, name);
        }
    }

    #[test]
    fn debt_types_stay_grey_and_labelled_honestly() {
        let untyped = note("a", NoteCategory::Permanent);
        let unknown = typed("b", NoteType::Unknown("concpet".to_string()));
        let time_scale = typed("c", NoteType::Daily);
        let drawn =
            cards(&[untyped, unknown, time_scale], &empty_positions(), TODAY);
        assert_eq!(drawn[0].bar, "bar-untyped");
        assert_eq!(drawn[0].label, "untyped");
        // the unknown name is shown verbatim — honest debt — but mints no class
        assert_eq!(drawn[1].bar, "bar-untyped");
        assert_eq!(drawn[1].label, "concpet");
        assert_eq!(drawn[2].bar, "bar-untyped");
        assert_eq!(drawn[2].label, "daily");
    }

    #[test]
    fn captures_wear_their_age_in_the_label() {
        let mut fresh = note("a", NoteCategory::Capture);
        fresh.created = Some("2026-07-23".to_string());
        let mut old = note("b", NoteCategory::Capture);
        old.created = Some("2026-07-20".to_string());
        let drawn = cards(&[fresh, old], &empty_positions(), TODAY);
        assert_eq!(drawn[0].label, "capture · 0 d");
        assert_eq!(drawn[0].bar, "bar-untyped");
        assert_eq!(drawn[1].label, "capture · 3 d");
    }

    #[test]
    fn a_capture_without_a_readable_date_is_just_capture() {
        let dateless = note("a", NoteCategory::Capture);
        let mut garbled = note("b", NoteCategory::Capture);
        garbled.created = Some("not-a-date".to_string());
        let drawn = cards(&[dateless, garbled], &empty_positions(), TODAY);
        assert_eq!(drawn[0].label, "capture");
        assert_eq!(drawn[1].label, "capture");
    }

    #[test]
    fn a_future_dated_capture_reads_as_today_not_negative() {
        let mut tomorrow = note("a", NoteCategory::Capture);
        tomorrow.created = Some("2026-07-24".to_string());
        let drawn = cards(&[tomorrow], &empty_positions(), TODAY);
        assert_eq!(drawn[0].label, "capture · 0 d");
    }

    #[test]
    fn generated_notes_carry_their_own_label_and_bar() {
        let drawn = cards(
            &[note("a", NoteCategory::Generated)],
            &empty_positions(),
            TODAY,
        );
        assert_eq!(drawn[0].label, "generated");
        assert_eq!(drawn[0].bar, "bar-generated");
        assert_eq!(drawn[0].kind, NoteCategory::Generated);
    }

    #[test]
    fn a_titleless_card_falls_back_to_its_id() {
        let mut titled = note("titled", NoteCategory::Permanent);
        titled.title = Some("A real title".to_string());
        let bare = note("bare", NoteCategory::Permanent);
        let drawn = cards(&[titled, bare], &empty_positions(), TODAY);
        assert_eq!(drawn[0].title, "A real title");
        assert_eq!(drawn[1].title, "bare");
    }

    #[test]
    fn a_placed_note_takes_its_stored_position() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("positions");
        let mut positions = Positions::load(&path);
        positions.set("a", 340.0, -120.5);
        let drawn =
            cards(&[note("a", NoteCategory::Permanent)], &positions, TODAY);
        assert_eq!((drawn[0].x, drawn[0].y), (340.0, -120.5));
    }

    #[test]
    fn unplaced_notes_stack_on_the_origin_grid_in_input_order() {
        let notes: Vec<TableNote> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| note(id, NoteCategory::Permanent))
            .collect();
        let drawn = cards(&notes, &empty_positions(), TODAY);
        let slots: Vec<(f64, f64)> =
            drawn.iter().map(|card| (card.x, card.y)).collect();
        assert_eq!(
            slots,
            vec![
                (32.0, 32.0),
                (32.0 + (CARD_WIDTH + 16.0), 32.0),
                (32.0 + 2.0 * (CARD_WIDTH + 16.0), 32.0),
                (32.0 + 3.0 * (CARD_WIDTH + 16.0), 32.0),
                // the fifth wraps to the second row
                (32.0, 128.0),
            ]
        );
        // deterministic: the same input stacks identically again
        assert_eq!(drawn, cards(&notes, &empty_positions(), TODAY));
    }

    #[test]
    fn rank_counts_only_unplaced_notes() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("positions");
        let mut positions = Positions::load(&path);
        positions.set("a", 500.0, 500.0);
        let notes = [
            note("a", NoteCategory::Permanent),
            note("b", NoteCategory::Permanent),
        ];
        let drawn = cards(&notes, &positions, TODAY);
        // b is the first unplaced note, so it takes the grid's first slot
        assert_eq!((drawn[1].x, drawn[1].y), (32.0, 32.0));
    }
}
