//! Fixture-driven tests: parsing the phase-1 sample vault yields the expected
//! notes and links (phase-2 exit criterion).

use jiff::civil::date;
use note_system::domain::{
    Link, Meta, MetaAnomaly, MetaStatus, NoteId, NoteType,
};
use note_system::parse::{ParsedNote, parse_note};

#[test]
fn happy_path_extracts_full_meta_and_links_in_source_order() {
    let parsed = parse_fixture("permanent/zettelkasten.typ");
    assert_eq!(
        parsed.meta,
        MetaStatus::Present(Meta {
            id: Some(NoteId("zettelkasten".to_string())),
            note_type: Some(NoteType::Concept),
            created: Some(date(2026, 7, 21)),
            tags: vec!["method".to_string()],
            origin: None,
            anomalies: vec![],
        })
    );
    assert_eq!(parsed.links, links(&["luhmann", "atomic-notes"]));
}

#[test]
fn links_in_comments_strings_and_raw_do_not_count() {
    let parsed = parse_fixture("permanent/link-traps.typ");
    assert_eq!(parsed.links, links(&["zettelkasten"]));
}

#[test]
fn missing_meta_is_data_not_an_error() {
    let parsed = parse_fixture("permanent/missing-meta.typ");
    assert_eq!(parsed.meta, MetaStatus::Missing);
    assert_eq!(parsed.links, vec![]);
}

#[test]
fn absent_type_field_is_none_without_anomaly() {
    let meta = present(parse_fixture("permanent/missing-type.typ"));
    assert_eq!(meta.note_type, None);
    assert_eq!(meta.anomalies, vec![]);
    assert_eq!(meta.id, Some(NoteId("missing-type".to_string())));
}

#[test]
fn duplicate_meta_first_wins_and_is_flagged() {
    let meta = present(parse_fixture("permanent/duplicate-meta.typ"));
    assert_eq!(meta.id, Some(NoteId("duplicate-meta".to_string())));
    assert_eq!(meta.note_type, Some(NoteType::Concept));
    assert_eq!(meta.anomalies, vec![MetaAnomaly::DuplicateMeta]);
}

#[test]
fn daily_note_empty_tags_array_and_ordered_prev_next_links() {
    let parsed = parse_fixture("time/2026-07-22.typ");
    let meta = present_ref(&parsed);
    assert_eq!(meta.note_type, Some(NoteType::Daily));
    assert_eq!(meta.tags, Vec::<String>::new());
    assert_eq!(meta.anomalies, vec![]);
    assert_eq!(parsed.links, links(&["2026-07-21", "2026-07-23"]));
}

#[test]
fn origin_field_is_extracted() {
    let meta = present(parse_fixture("generated/digest-smart-notes.typ"));
    assert_eq!(meta.origin, Some("smart-notes".to_string()));
    assert_eq!(meta.note_type, Some(NoteType::Generated));
}

#[test]
fn capture_note_has_no_type_by_design() {
    let meta = present(parse_fixture("capture/capture-idea-canvas.typ"));
    assert_eq!(meta.note_type, None);
    assert_eq!(meta.anomalies, vec![]);
}

fn parse_fixture(relative: &str) -> ParsedNote {
    let path = format!(
        "{}/tests/fixtures/vault/{}",
        env!("CARGO_MANIFEST_DIR"),
        relative
    );
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {path}: {e}"));
    parse_note(&source)
}

fn present(parsed: ParsedNote) -> Meta {
    match parsed.meta {
        MetaStatus::Present(meta) => meta,
        MetaStatus::Missing => panic!("expected a #meta call, found none"),
    }
}

fn present_ref(parsed: &ParsedNote) -> &Meta {
    match &parsed.meta {
        MetaStatus::Present(meta) => meta,
        MetaStatus::Missing => panic!("expected a #meta call, found none"),
    }
}

fn links(targets: &[&str]) -> Vec<Link> {
    targets
        .iter()
        .map(|t| Link {
            target: NoteId(t.to_string()),
        })
        .collect()
}
