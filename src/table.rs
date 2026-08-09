//! Pure logic behind the table screen: the card each indexed note becomes,
//! the treatment its kind wears, and where an unplaced note stands until
//! phase 8 places it. Everything decidable without a VirtualDom lives here,
//! so the component stays wiring (adr/2026-07-ui-covered-at-100.md).

use std::collections::HashMap;

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

/// Where the unplaced stand this session: an id keeps the first slot it was
/// given until a real position exists for it, so placing one card never
/// shuffles the rest mid-session. The store stays untouched — a missing
/// entry still means "not yet placed", and phase 8 still gets to decide
/// (adr/2026-08-table-mounts-titles-zoom.md).
#[derive(Default)]
pub struct Fallback {
    slots: HashMap<String, (f64, f64)>,
    next: usize,
}

impl Fallback {
    /// The remembered slot, or the next one near the origin for a new note.
    /// Slots are never reclaimed within a session: reuse would put a fresh
    /// card exactly where a just-placed one appeared to leave from.
    fn slot_for(&mut self, id: &str) -> (f64, f64) {
        if let Some(slot) = self.slots.get(id) {
            return *slot;
        }
        let slot = fallback_slot(self.next);
        self.next += 1;
        self.slots.insert(id.to_string(), slot);
        slot
    }
}

/// Resolve every table note against the store. Unplaced notes take a
/// session-stable slot near the origin — dumb and honest until phase 8
/// places them (roadmap-v1.md § Phase 2).
pub fn cards(
    notes: &[TableNote],
    positions: &Positions,
    fallback: &mut Fallback,
    today: Date,
) -> Vec<Card> {
    notes
        .iter()
        .map(|note| {
            let (x, y) = positions
                .get(&note.id)
                .unwrap_or_else(|| fallback.slot_for(&note.id));
            let kind = presented_kind(note);
            Card {
                id: note.id.clone(),
                title: note.title.clone().unwrap_or_else(|| note.id.clone()),
                label: label(kind, note, today),
                kind,
                bar: bar_class(kind, note),
                x,
                y,
            }
        })
        .collect()
}

/// The kind the card is drawn as. Presentation keys on the type: a capture
/// that gained one of the eight permanent types is promoted on sight — hue,
/// full fill, type label — while the file stays in `capture/` and the index
/// category stays honest (adr/2026-08-typed-capture-wears-its-hue.md).
fn presented_kind(note: &TableNote) -> NoteCategory {
    match note.kind {
        NoteCategory::Capture
            if note.note_type.as_ref().is_some_and(NoteType::is_permanent) =>
        {
            NoteCategory::Permanent
        }
        kind => kind,
    }
}

const GRID_COLUMNS: usize = 4;
const GRID_ORIGIN: f64 = 32.0;
const GRID_GAP: f64 = 16.0;
const GRID_ROW_HEIGHT: f64 = 96.0;

/// The slot generator: a 4-wide grid at the canvas origin, visible where
/// the viewport starts. The grid is only a spacing scheme — what matters is
/// that new cards land near the origin without covering each other.
fn fallback_slot(rank: usize) -> (f64, f64) {
    let column = (rank % GRID_COLUMNS) as f64;
    let row = (rank / GRID_COLUMNS) as f64;
    (
        GRID_ORIGIN + column * (CARD_WIDTH + GRID_GAP),
        GRID_ORIGIN + row * GRID_ROW_HEIGHT,
    )
}

/// The card's label line, keyed on the presented kind. The query never
/// yields time notes, so the Time arm only keeps the match total — it wears
/// the permanent treatment rather than a panic no test could reach.
fn label(kind: NoteCategory, note: &TableNote, today: Date) -> String {
    match kind {
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

/// The sheet's fixed frame (wireframe state 6b: the panel from x=440; width
/// is the deck's `sheetW` knob at its default), in viewport coordinates —
/// the sheet never pans, the tether bridges the two spaces.
pub const SHEET_LEFT: f64 = 440.0;
pub const SHEET_WIDTH: f64 = 620.0;
/// Where the tether meets the card: its mid-height at titles zoom.
pub const TETHER_DROP: f64 = 28.0;
/// How far a press may wander and still read as a click (max-norm, px).
pub const CLICK_SLOP: f64 = 4.0;

/// The viewport a headless run assumes when no window injects its real
/// size — deterministic, never an error
/// (adr/2026-08-new-card-lands-at-viewport-centre.md).
pub const DEFAULT_VIEWPORT: (f64, f64) = (1280.0, 800.0);

/// Canvas coordinates that centre a new card in the viewport under `pan`:
/// where the user is looking is where the note appears
/// (adr/2026-08-new-card-lands-at-viewport-centre.md). The card's nominal
/// mid-height is the tether's drop, the one height the layout declares.
pub fn spawn_position(viewport: (f64, f64), pan: (f64, f64)) -> (f64, f64) {
    (
        viewport.0 / 2.0 - CARD_WIDTH / 2.0 - pan.0,
        viewport.1 / 2.0 - TETHER_DROP - pan.1,
    )
}

/// The tether's box: a horizontal line at the card's mid-height, from the
/// card's nearest edge to the sheet's, in viewport coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Tether {
    pub left: f64,
    pub top: f64,
    pub width: f64,
}

/// Where the tether runs for a card at canvas (x, y) under the current pan.
/// A card left of the sheet tethers from its right edge; right of it, from
/// the sheet's right edge; a card overlapping the sheet collapses to zero
/// width — drawn as nothing rather than branched around.
pub fn tether(card_x: f64, card_y: f64, pan: (f64, f64)) -> Tether {
    let left_edge = card_x + pan.0;
    let right_edge = left_edge + CARD_WIDTH;
    let top = card_y + pan.1 + TETHER_DROP;
    if right_edge <= SHEET_LEFT {
        Tether {
            left: right_edge,
            top,
            width: SHEET_LEFT - right_edge,
        }
    } else {
        let sheet_right = SHEET_LEFT + SHEET_WIDTH;
        Tether {
            left: sheet_right,
            top,
            width: (left_edge - sheet_right).max(0.0),
        }
    }
}

/// A press that never wandered past the slop is a click, not a drag —
/// decided at mouseup, so a real drag can still end anywhere it likes.
pub fn is_click(down: (f64, f64), up: (f64, f64)) -> bool {
    (up.0 - down.0).abs() <= CLICK_SLOP && (up.1 - down.1).abs() <= CLICK_SLOP
}

/// The 3px bar, keyed on the presented kind: eight permanent hues;
/// everything else — captures, unknown or time-scale types, no type at all
/// — is the grey of visible debt, and generated notes keep their own dashed
/// bar.
fn bar_class(kind: NoteCategory, note: &TableNote) -> &'static str {
    match kind {
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
            let drawn = cards(
                &[typed("a", note_type)],
                &empty_positions(),
                &mut Fallback::default(),
                TODAY,
            );
            assert_eq!(drawn[0].bar, expected, "for type {name}");
            assert_eq!(drawn[0].label, name);
        }
    }

    #[test]
    fn debt_types_stay_grey_and_labelled_honestly() {
        let untyped = note("a", NoteCategory::Permanent);
        let unknown = typed("b", NoteType::Unknown("concpet".to_string()));
        let time_scale = typed("c", NoteType::Daily);
        let drawn = cards(
            &[untyped, unknown, time_scale],
            &empty_positions(),
            &mut Fallback::default(),
            TODAY,
        );
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
        let drawn = cards(
            &[fresh, old],
            &empty_positions(),
            &mut Fallback::default(),
            TODAY,
        );
        assert_eq!(drawn[0].label, "capture · 0 d");
        assert_eq!(drawn[0].bar, "bar-untyped");
        assert_eq!(drawn[1].label, "capture · 3 d");
    }

    #[test]
    fn a_capture_without_a_readable_date_is_just_capture() {
        let dateless = note("a", NoteCategory::Capture);
        let mut garbled = note("b", NoteCategory::Capture);
        garbled.created = Some("not-a-date".to_string());
        let drawn = cards(
            &[dateless, garbled],
            &empty_positions(),
            &mut Fallback::default(),
            TODAY,
        );
        assert_eq!(drawn[0].label, "capture");
        assert_eq!(drawn[1].label, "capture");
    }

    #[test]
    fn a_future_dated_capture_reads_as_today_not_negative() {
        let mut tomorrow = note("a", NoteCategory::Capture);
        tomorrow.created = Some("2026-07-24".to_string());
        let drawn = cards(
            &[tomorrow],
            &empty_positions(),
            &mut Fallback::default(),
            TODAY,
        );
        assert_eq!(drawn[0].label, "capture · 0 d");
    }

    #[test]
    fn generated_notes_carry_their_own_label_and_bar() {
        let drawn = cards(
            &[note("a", NoteCategory::Generated)],
            &empty_positions(),
            &mut Fallback::default(),
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
        let drawn = cards(
            &[titled, bare],
            &empty_positions(),
            &mut Fallback::default(),
            TODAY,
        );
        assert_eq!(drawn[0].title, "A real title");
        assert_eq!(drawn[1].title, "bare");
    }

    #[test]
    fn a_placed_note_takes_its_stored_position() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let path = dir.path().join("positions");
        let mut positions = Positions::load(&path);
        positions.set("a", 340.0, -120.5);
        let drawn = cards(
            &[note("a", NoteCategory::Permanent)],
            &positions,
            &mut Fallback::default(),
            TODAY,
        );
        assert_eq!((drawn[0].x, drawn[0].y), (340.0, -120.5));
    }

    #[test]
    fn unplaced_notes_stack_on_the_origin_grid_in_input_order() {
        let notes: Vec<TableNote> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| note(id, NoteCategory::Permanent))
            .collect();
        let drawn =
            cards(&notes, &empty_positions(), &mut Fallback::default(), TODAY);
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
        assert_eq!(
            drawn,
            cards(&notes, &empty_positions(), &mut Fallback::default(), TODAY)
        );
    }

    #[test]
    fn placing_one_card_leaves_the_others_standing() {
        let mut fallback = Fallback::default();
        let notes = [
            note("a", NoteCategory::Permanent),
            note("b", NoteCategory::Permanent),
        ];
        let before = cards(&notes, &empty_positions(), &mut fallback, TODAY);
        assert_eq!((before[1].x, before[1].y), (224.0, 32.0));

        // a gets dragged somewhere real; b must not compact into its slot
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut positions = Positions::load(&dir.path().join("positions"));
        positions.set("a", 900.0, 900.0);
        let after = cards(&notes, &positions, &mut fallback, TODAY);
        assert_eq!((after[0].x, after[0].y), (900.0, 900.0));
        assert_eq!(
            (after[1].x, after[1].y),
            (224.0, 32.0),
            "the untouched card kept the slot it was first given"
        );
    }

    #[test]
    fn a_card_left_of_the_sheet_tethers_from_its_right_edge() {
        let drawn = tether(100.0, 200.0, (0.0, 0.0));
        assert_eq!(
            drawn,
            Tether {
                left: 100.0 + CARD_WIDTH,
                top: 200.0 + TETHER_DROP,
                width: SHEET_LEFT - (100.0 + CARD_WIDTH),
            }
        );
    }

    #[test]
    fn the_pan_moves_the_tether_with_the_card() {
        let still = tether(100.0, 200.0, (0.0, 0.0));
        let panned = tether(100.0, 200.0, (40.0, -16.0));
        assert_eq!(panned.left, still.left + 40.0);
        assert_eq!(panned.top, still.top - 16.0);
        // the sheet stands still while the card slides toward it
        assert_eq!(panned.width, still.width - 40.0);
    }

    #[test]
    fn a_card_right_of_the_sheet_tethers_from_the_sheets_edge() {
        let drawn = tether(1200.0, 60.0, (0.0, 0.0));
        assert_eq!(drawn.left, SHEET_LEFT + SHEET_WIDTH);
        assert_eq!(drawn.width, 1200.0 - (SHEET_LEFT + SHEET_WIDTH));
        assert_eq!(drawn.top, 60.0 + TETHER_DROP);
    }

    #[test]
    fn a_card_overlapping_the_sheet_draws_no_tether() {
        // straddling the sheet's left edge
        let straddling =
            tether(SHEET_LEFT - CARD_WIDTH / 2.0, 0.0, (0.0, 0.0));
        assert_eq!(straddling.width, 0.0);
        // and fully under it
        let under = tether(SHEET_LEFT + 40.0, 0.0, (0.0, 0.0));
        assert_eq!(under.width, 0.0);
    }

    #[test]
    fn the_spawn_position_centres_a_card_under_the_pan() {
        // an unpanned 1280×800 viewport: the card's left edge sits half a
        // card left of centre, its mid-height (the tether drop) at mid-height
        assert_eq!(
            spawn_position(DEFAULT_VIEWPORT, (0.0, 0.0)),
            (640.0 - CARD_WIDTH / 2.0, 400.0 - TETHER_DROP)
        );
        // a panned canvas compensates: the card still lands mid-viewport
        assert_eq!(
            spawn_position(DEFAULT_VIEWPORT, (100.0, -60.0)),
            (640.0 - CARD_WIDTH / 2.0 - 100.0, 400.0 - TETHER_DROP + 60.0)
        );
    }

    #[test]
    fn a_typed_capture_wears_its_hue_and_full_fill() {
        let mut promoted = note("a", NoteCategory::Capture);
        promoted.note_type = Some(NoteType::Concept);
        promoted.created = Some("2026-07-20".to_string());
        let drawn = cards(
            &[promoted],
            &empty_positions(),
            &mut Fallback::default(),
            TODAY,
        );
        // the one kind switch carries the label, the bar and (through
        // card-{kind}) the full fill (adr/2026-08-typed-capture-wears-its-hue.md)
        assert_eq!(drawn[0].kind, NoteCategory::Permanent);
        assert_eq!(drawn[0].bar, "bar-concept");
        assert_eq!(drawn[0].label, "concept");
    }

    #[test]
    fn an_unknown_typed_capture_stays_grey_capture() {
        let mut garbled = note("a", NoteCategory::Capture);
        garbled.note_type = Some(NoteType::Unknown("concpet".to_string()));
        garbled.created = Some("2026-07-20".to_string());
        let drawn = cards(
            &[garbled],
            &empty_positions(),
            &mut Fallback::default(),
            TODAY,
        );
        // an unrecognized type is not a promotion — the age stays visible debt
        assert_eq!(drawn[0].kind, NoteCategory::Capture);
        assert_eq!(drawn[0].bar, "bar-untyped");
        assert_eq!(drawn[0].label, "capture · 3 d");
    }

    #[test]
    fn a_press_inside_the_slop_is_a_click_and_beyond_it_a_drag() {
        assert!(is_click((10.0, 10.0), (10.0, 10.0)));
        assert!(is_click((10.0, 10.0), (14.0, 6.0)));
        assert!(!is_click((10.0, 10.0), (14.1, 10.0)));
        assert!(!is_click((10.0, 10.0), (10.0, 15.0)));
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
        let drawn = cards(&notes, &positions, &mut Fallback::default(), TODAY);
        // b is the first unplaced note, so it takes the grid's first slot
        assert_eq!((drawn[1].x, drawn[1].y), (32.0, 32.0));
    }
}
