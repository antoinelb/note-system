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
    /// Vault-relative — what body zoom renders and caches by
    /// (adr/2026-08-body-cache-per-note-svg.md).
    pub path: std::path::PathBuf,
    /// Falls back to the id — a card is never blank.
    pub title: String,
    /// The uppercase mono line over the title: the type name, or the
    /// capture's age ("capture · 3 d", plan.md § Friction system).
    pub label: String,
    pub kind: NoteCategory,
    /// The 3px bar's CSS class, one of a closed set — never minted from a
    /// type name, so an unknown type cannot invent a class no variable backs.
    pub bar: &'static str,
    /// Filtered out: the card dims, never disappears — spatial memory is
    /// the point (adr/2026-08-filter-overlay-ctrl-f.md).
    pub dimmed: bool,
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
/// places them (roadmap-v1.md § Phase 2). A filter dims, never drops
/// (adr/2026-08-filter-overlay-ctrl-f.md).
pub fn cards(
    notes: &[TableNote],
    positions: &Positions,
    fallback: &mut Fallback,
    filter: Option<&Filter>,
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
                path: note.path.clone(),
                title: note.title.clone().unwrap_or_else(|| note.id.clone()),
                label: label(kind, note, today),
                kind,
                bar: bar_class(kind, note),
                dimmed: filter.is_some_and(|filter| !matches(note, filter)),
                x,
                y,
            }
        })
        .collect()
}

/// One active filter — at most one at a time: applying replaces the last
/// (adr/2026-08-filter-overlay-ctrl-f.md).
#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    Tag(String),
    Type(NoteType),
}

/// The chrome's label for the active filter: the map reads differently
/// under one, and the chrome must say why.
pub fn filter_label(filter: &Filter) -> String {
    match filter {
        Filter::Tag(tag) => format!("tag · {tag}"),
        Filter::Type(note_type) => {
            format!("type · {}", note_type.as_name())
        }
    }
}

/// Whether the note survives the filter. Matching keys on the note's own
/// meta, not the presented kind: a type filter dims captures, generated
/// and the untyped too — they are not the type asked for.
fn matches(note: &TableNote, filter: &Filter) -> bool {
    match filter {
        Filter::Tag(tag) => note.tags.iter().any(|own| own == tag),
        Filter::Type(wanted) => note.note_type.as_ref() == Some(wanted),
    }
}

/// One row the filter overlay offers.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterEntry {
    pub label: String,
    pub filter: Filter,
}

/// The overlay's full vocabulary: every tag, then the eight permanent
/// types — a closed list an empty query shows whole.
pub fn filter_entries(tags: &[String]) -> Vec<FilterEntry> {
    tags.iter()
        .map(|tag| FilterEntry {
            label: tag.clone(),
            filter: Filter::Tag(tag.clone()),
        })
        .chain(crate::create::TYPES.iter().map(|note_type| FilterEntry {
            label: note_type.as_name().to_string(),
            filter: Filter::Type(note_type.clone()),
        }))
        .collect()
}

/// The rows a query leaves — the palette's contains rule.
pub fn filter_rows<'entries>(
    query: &str,
    entries: &'entries [FilterEntry],
) -> Vec<&'entries FilterEntry> {
    let needle = query.to_lowercase();
    entries
        .iter()
        .filter(|entry| entry.label.to_lowercase().contains(&needle))
        .collect()
}

/// The pan that puts a card's nominal centre at the viewport centre at
/// this zoom: pan = centre/s − card_centre
/// (adr/2026-08-jump-ctrl-o-centres-viewport.md).
pub fn centre_on(
    card: &Card,
    zoom: Zoom,
    viewport: (f64, f64),
) -> (f64, f64) {
    let scale = zoom.scale();
    (
        viewport.0 / 2.0 / scale - (card.x + CARD_WIDTH / 2.0),
        viewport.1 / 2.0 / scale - (card.y + TETHER_DROP),
    )
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

/// The two semantic zoom levels (plan.md § Canvas): titles, and rendered
/// bodies at three times the size
/// (adr/2026-08-body-zoom-scale-and-metrics.md).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zoom {
    Titles,
    Bodies,
}

impl Zoom {
    /// The canvas's scale factor — applied outside the translate, so the
    /// pan stays in canvas units and `point()` divides once.
    pub fn scale(self) -> f64 {
        match self {
            Zoom::Titles => 1.0,
            Zoom::Bodies => 3.0,
        }
    }
}

/// The clipped body area below label and title, and the card's full height
/// at body zoom (adr/2026-08-body-zoom-scale-and-metrics.md).
pub const BODY_HEIGHT: f64 = 240.0;
pub const BODY_CARD_HEIGHT: f64 = 296.0;
/// A conservative title-card height bound for culling — cards are shorter,
/// and a too-tall bound only keeps an off-screen card alive, never culls a
/// visible one.
pub const TITLE_CARD_HEIGHT: f64 = 96.0;

/// Whether the card's rectangle intersects the viewport under
/// `scale(zoom) translate(pan)`: screen = s·(canvas + pan). Exact edge
/// contact does not count — a card ending at the boundary shows nothing
/// (adr/2026-08-viewport-culling-onresize.md).
pub fn in_view(
    card: &Card,
    zoom: Zoom,
    pan: (f64, f64),
    viewport: (f64, f64),
) -> bool {
    let scale = zoom.scale();
    let height = match zoom {
        Zoom::Titles => TITLE_CARD_HEIGHT,
        Zoom::Bodies => BODY_CARD_HEIGHT,
    };
    let left = scale * (card.x + pan.0);
    let top = scale * (card.y + pan.1);
    left < viewport.0
        && left + scale * CARD_WIDTH > 0.0
        && top < viewport.1
        && top + scale * height > 0.0
}

/// The pan that keeps the canvas point under the viewport centre fixed
/// across a zoom change: p = centre/s − pan, so
/// pan' = pan + centre·(1/s' − 1/s)
/// (adr/2026-08-body-zoom-scale-and-metrics.md).
pub fn rezoom(
    pan: (f64, f64),
    from: Zoom,
    to: Zoom,
    viewport: (f64, f64),
) -> (f64, f64) {
    let shift = 1.0 / to.scale() - 1.0 / from.scale();
    (
        pan.0 + viewport.0 / 2.0 * shift,
        pan.1 + viewport.1 / 2.0 * shift,
    )
}

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

/// One drawn edge in canvas coordinates, endpoints already clipped to the
/// card borders — where the line runs and where its node dots sit
/// (adr/2026-08-edges-svg-under-cards.md).
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// The card's nominal half-extents: half a card wide, the tether's drop
/// tall — the one card height the layout declares.
const EDGE_HALF_WIDTH: f64 = CARD_WIDTH / 2.0;
const EDGE_HALF_HEIGHT: f64 = TETHER_DROP;

/// The link index resolved against the drawn cards: an edge for every pair
/// whose ends both stand on the table, centre to centre, clipped to each
/// card's border. A lookup miss — dangling, a time note, an id the table
/// does not host — draws nothing, as does a self-link or an overlapping
/// pair (the tether's zero-width idiom).
pub fn edges(links: &[(String, String)], cards: &[Card]) -> Vec<Edge> {
    let by_id: HashMap<&str, &Card> =
        cards.iter().map(|card| (card.id.as_str(), card)).collect();
    links
        .iter()
        .filter_map(|(source, target)| {
            let from = by_id.get(source.as_str())?;
            let to = by_id.get(target.as_str())?;
            let (x1, y1, x2, y2) = clip(from, to)?;
            Some(Edge {
                source: source.clone(),
                target: target.clone(),
                x1,
                y1,
                x2,
                y2,
            })
        })
        .collect()
}

/// Centre to centre, each end pulled in to its card's border: the exit
/// parameter from the source rectangle mirrors the entry into the target's
/// along the one segment, both cards being the same size. Crossed clips
/// mean the cards overlap — nothing to draw. Zero-length axes need no
/// branch: dividing by zero yields infinity, which loses every `min`, and
/// a self-link's two infinities cross like any other overlap.
fn clip(from: &Card, to: &Card) -> Option<(f64, f64, f64, f64)> {
    let (x1, y1) = (from.x + EDGE_HALF_WIDTH, from.y + EDGE_HALF_HEIGHT);
    let (x2, y2) = (to.x + EDGE_HALF_WIDTH, to.y + EDGE_HALF_HEIGHT);
    let (dx, dy) = (x2 - x1, y2 - y1);
    let reach = (EDGE_HALF_WIDTH / dx.abs()).min(EDGE_HALF_HEIGHT / dy.abs());
    let entry = 1.0 - reach;
    if reach >= entry {
        return None;
    }
    Some((
        x1 + reach * dx,
        y1 + reach * dy,
        x1 + entry * dx,
        y1 + entry * dy,
    ))
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
            path: std::path::PathBuf::from(format!(
                "{}/{id}.typ",
                kind.as_dir()
            )),
            kind,
            note_type: None,
            title: None,
            created: None,
            tags: Vec::new(),
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
                None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            None,
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
            cards(&notes, &empty_positions(), &mut Fallback::default(), None, TODAY);
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
            cards(&notes, &empty_positions(), &mut Fallback::default(), None, TODAY)
        );
    }

    #[test]
    fn placing_one_card_leaves_the_others_standing() {
        let mut fallback = Fallback::default();
        let notes = [
            note("a", NoteCategory::Permanent),
            note("b", NoteCategory::Permanent),
        ];
        let before = cards(&notes, &empty_positions(), &mut fallback, None, TODAY);
        assert_eq!((before[1].x, before[1].y), (224.0, 32.0));

        // a gets dragged somewhere real; b must not compact into its slot
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut positions = Positions::load(&dir.path().join("positions"));
        positions.set("a", 900.0, 900.0);
        let after = cards(&notes, &positions, &mut fallback, None, TODAY);
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
            None,
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
            None,
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
    fn a_tag_filter_dims_the_cards_without_that_tag() {
        let mut tagged = typed("a", NoteType::Concept);
        tagged.tags = vec!["method".to_string()];
        let plain = typed("b", NoteType::Concept);
        let filter = Filter::Tag("method".to_string());
        let drawn = cards(
            &[tagged, plain],
            &empty_positions(),
            &mut Fallback::default(),
            Some(&filter),
            TODAY,
        );
        assert!(!drawn[0].dimmed);
        assert!(drawn[1].dimmed, "no tag, dimmed — but still drawn");
    }

    #[test]
    fn a_type_filter_dims_captures_and_the_untyped_too() {
        let wanted = typed("a", NoteType::Concept);
        let other = typed("b", NoteType::Idea);
        let untyped = note("c", NoteCategory::Permanent);
        let capture = note("d", NoteCategory::Capture);
        let generated = note("e", NoteCategory::Generated);
        let filter = Filter::Type(NoteType::Concept);
        let drawn = cards(
            &[wanted, other, untyped, capture, generated],
            &empty_positions(),
            &mut Fallback::default(),
            Some(&filter),
            TODAY,
        );
        let dimmed: Vec<bool> =
            drawn.iter().map(|card| card.dimmed).collect();
        assert_eq!(dimmed, vec![false, true, true, true, true]);
    }

    #[test]
    fn no_filter_dims_nothing() {
        let drawn = cards(
            &[note("a", NoteCategory::Permanent)],
            &empty_positions(),
            &mut Fallback::default(),
            None,
            TODAY,
        );
        assert!(!drawn[0].dimmed);
    }

    #[test]
    fn filter_entries_list_every_tag_then_the_eight_types() {
        let entries = filter_entries(&[
            "method".to_string(),
            "zettel".to_string(),
        ]);
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0].label, "method");
        assert_eq!(
            entries[0].filter,
            Filter::Tag("method".to_string())
        );
        assert_eq!(entries[2].label, "person");
        assert_eq!(entries[2].filter, Filter::Type(NoteType::Person));
        assert_eq!(entries[9].filter, Filter::Type(NoteType::Project));
    }

    #[test]
    fn the_filter_query_narrows_tags_and_types_together() {
        let entries = filter_entries(&["personnel".to_string()]);
        let rows: Vec<&str> = filter_rows("PERSON", &entries)
            .iter()
            .map(|entry| entry.label.as_str())
            .collect();
        // the tag and both matching types, ignoring case
        assert_eq!(rows, vec!["personnel", "person", "personal"]);
        assert_eq!(filter_rows("xyzzy", &entries), Vec::<&FilterEntry>::new());
    }

    #[test]
    fn filter_label_names_its_kind() {
        assert_eq!(
            filter_label(&Filter::Tag("method".to_string())),
            "tag · method"
        );
        assert_eq!(
            filter_label(&Filter::Type(NoteType::Concept)),
            "type · concept"
        );
    }

    #[test]
    fn centre_on_puts_the_card_centre_mid_viewport_at_both_zooms() {
        let card = placed_card("a", 32.0, 32.0);
        let vp = (1280.0, 800.0);
        // titles: pan = centre − card centre (88 + 32, 28 + 32)
        assert_eq!(
            centre_on(&card, Zoom::Titles, vp),
            (640.0 - 120.0, 400.0 - 60.0)
        );
        // bodies: the viewport centre lives at centre/s in canvas units
        assert_eq!(
            centre_on(&card, Zoom::Bodies, vp),
            (640.0 / 3.0 - 120.0, 400.0 / 3.0 - 60.0)
        );
    }

    fn placed_card(id: &str, x: f64, y: f64) -> Card {
        Card {
            id: id.to_string(),
            path: std::path::PathBuf::from(format!("permanent/{id}.typ")),
            title: id.to_string(),
            label: "concept".to_string(),
            kind: NoteCategory::Permanent,
            bar: "bar-concept",
            dimmed: false,
            x,
            y,
        }
    }

    fn link(source: &str, target: &str) -> (String, String) {
        (source.to_string(), target.to_string())
    }

    #[test]
    fn zoom_scales_are_titles_one_and_bodies_three() {
        assert_eq!(Zoom::Titles.scale(), 1.0);
        assert_eq!(Zoom::Bodies.scale(), 3.0);
    }

    #[test]
    fn a_card_leaves_view_exactly_at_each_boundary() {
        let card = placed_card("a", 100.0, 100.0);
        let vp = (1280.0, 800.0);
        assert!(in_view(&card, Zoom::Titles, (0.0, 0.0), vp));
        // exact edge contact counts as out, one pixel back in as in
        assert!(!in_view(&card, Zoom::Titles, (1180.0, 0.0), vp), "left");
        assert!(in_view(&card, Zoom::Titles, (1179.0, 0.0), vp));
        assert!(!in_view(&card, Zoom::Titles, (-276.0, 0.0), vp), "right");
        assert!(in_view(&card, Zoom::Titles, (-275.0, 0.0), vp));
        assert!(!in_view(&card, Zoom::Titles, (0.0, 700.0), vp), "top");
        assert!(in_view(&card, Zoom::Titles, (0.0, 699.0), vp));
        assert!(!in_view(&card, Zoom::Titles, (0.0, -196.0), vp), "bottom");
        assert!(in_view(&card, Zoom::Titles, (0.0, -195.0), vp));
    }

    #[test]
    fn culling_respects_the_pan_and_the_scale() {
        let vp = (1280.0, 800.0);
        // the same pan puts a body-zoomed card thrice as far out
        let far = placed_card("a", 500.0, 0.0);
        assert!(in_view(&far, Zoom::Titles, (0.0, 0.0), vp));
        assert!(!in_view(&far, Zoom::Bodies, (0.0, 0.0), vp));
        // the taller body card survives higher above the fold
        let high = placed_card("b", 0.0, -290.0);
        assert!(!in_view(&high, Zoom::Titles, (0.0, 0.0), vp));
        assert!(in_view(&high, Zoom::Bodies, (0.0, 0.0), vp));
        // and panning brings the far card back
        assert!(in_view(&far, Zoom::Bodies, (-200.0, 0.0), vp));
    }

    #[test]
    fn rezoom_keeps_the_viewport_centre_on_the_same_canvas_point() {
        let vp = (1280.0, 800.0);
        let pan = (-40.0, 40.0);
        let zoomed = rezoom(pan, Zoom::Titles, Zoom::Bodies, vp);
        // the canvas point under the centre: p = centre/s − pan
        let before = (640.0 - pan.0, 400.0 - pan.1);
        let after = (640.0 / 3.0 - zoomed.0, 400.0 / 3.0 - zoomed.1);
        assert!((before.0 - after.0).abs() < 1e-9, "{before:?} {after:?}");
        assert!((before.1 - after.1).abs() < 1e-9, "{before:?} {after:?}");
        // and back out is the identity round trip
        let back = rezoom(zoomed, Zoom::Bodies, Zoom::Titles, vp);
        assert!((back.0 - pan.0).abs() < 1e-9);
        assert!((back.1 - pan.1).abs() < 1e-9);
    }

    #[test]
    fn an_edge_runs_border_to_border_between_placed_cards() {
        // side by side: the line is horizontal, so each end sits on a
        // vertical border — the card's edge, not its centre
        let cards = [placed_card("a", 0.0, 0.0), placed_card("b", 400.0, 0.0)];
        let drawn = edges(&[link("a", "b")], &cards);
        assert_eq!(drawn.len(), 1);
        assert_eq!((drawn[0].x1, drawn[0].y1), (CARD_WIDTH, TETHER_DROP));
        assert_eq!((drawn[0].x2, drawn[0].y2), (400.0, TETHER_DROP));

        // stacked: the vertical borders take over — the other min arm
        let cards = [placed_card("a", 0.0, 0.0), placed_card("b", 0.0, 200.0)];
        let drawn = edges(&[link("a", "b")], &cards);
        assert_eq!(
            (drawn[0].x1, drawn[0].y1),
            (CARD_WIDTH / 2.0, 2.0 * TETHER_DROP),
            "out through the bottom border"
        );
        assert_eq!(
            (drawn[0].x2, drawn[0].y2),
            (CARD_WIDTH / 2.0, 200.0),
            "in through the top border"
        );
    }

    #[test]
    fn a_link_to_an_absent_or_unhosted_id_draws_nothing() {
        let cards = [placed_card("a", 0.0, 0.0)];
        // dangling target, and a source the table does not host
        assert_eq!(edges(&[link("a", "ghost")], &cards), vec![]);
        assert_eq!(edges(&[link("2026-07-23", "a")], &cards), vec![]);
    }

    #[test]
    fn a_self_link_draws_nothing() {
        let cards = [placed_card("a", 0.0, 0.0)];
        assert_eq!(edges(&[link("a", "a")], &cards), vec![]);
    }

    #[test]
    fn overlapping_cards_draw_no_edge() {
        // b starts inside a's nominal rectangle: the clips cross
        let cards =
            [placed_card("a", 0.0, 0.0), placed_card("b", 100.0, 10.0)];
        assert_eq!(edges(&[link("a", "b")], &cards), vec![]);
    }

    #[test]
    fn moving_a_card_moves_its_edge_endpoints() {
        let links = [link("a", "b")];
        let before = edges(
            &links,
            &[placed_card("a", 0.0, 0.0), placed_card("b", 400.0, 0.0)],
        );
        let after = edges(
            &links,
            &[placed_card("a", 0.0, 0.0), placed_card("b", 480.0, 0.0)],
        );
        assert_eq!(before[0].x1, after[0].x1, "the still end held");
        assert_eq!(
            after[0].x2,
            before[0].x2 + 80.0,
            "the dragged end followed"
        );
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
        let drawn = cards(&notes, &positions, &mut Fallback::default(), None, TODAY);
        // b is the first unplaced note, so it takes the grid's first slot
        assert_eq!((drawn[1].x, drawn[1].y), (32.0, 32.0));
    }
}
