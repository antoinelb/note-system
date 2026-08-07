use crate::domain::{Link, Meta, MetaAnomaly, MetaStatus, NoteId, NoteType};
use jiff::civil::Date;
use typst_syntax::ast::{self, AstNode};

pub(crate) const MAX_NODES: usize = 100_000;

pub struct ParsedNote {
    pub meta: MetaStatus,
    /// The note's first level-1 heading, the human name the link picker
    /// searches beside the id (adr/2026-08-titles-in-index.md). Content, not
    /// metadata — a note may have none.
    pub title: Option<String>,
    pub links: Vec<Link>,
}

pub fn parse_note(source: &str) -> ParsedNote {
    let root = typst_syntax::parse(source);
    let mut meta = MetaStatus::Missing;
    let mut title = None;
    let mut links = Vec::new();
    // children are pushed reversed, so popping walks the tree in document
    // order — what "the first heading wins" means
    let mut stack = vec![&root];

    for _ in 0..MAX_NODES {
        let Some(node) = stack.pop() else { break };
        if let Some(call) = node.cast::<ast::FuncCall>()
            && let ast::Expr::Ident(name) = call.callee()
        {
            match name.as_str() {
                "meta" => match &mut meta {
                    MetaStatus::Missing => {
                        meta = MetaStatus::Present(extract_meta(call))
                    }
                    MetaStatus::Present(m) => {
                        m.anomalies.push(MetaAnomaly::DuplicateMeta)
                    }
                },
                "l" => links.extend(
                    extract_link_target(call).map(|target| Link { target }),
                ),
                _ => {}
            }
        }
        if title.is_none()
            && let Some(heading) = node.cast::<ast::Heading>()
        {
            title = extract_title(heading);
        }
        stack.extend(node.children().rev());
    }

    ParsedNote { meta, title, links }
}

/// A level-1 heading's text; deeper headings are sections inside a note, not
/// its name, and a heading with nothing but whitespace names nothing.
fn extract_title(heading: ast::Heading) -> Option<String> {
    if heading.depth().get() != 1 {
        return None;
    }
    let text = heading.body().to_untyped().full_text();
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn extract_meta(call: ast::FuncCall) -> Meta {
    let mut meta = Meta::default();
    for arg in call.args().items() {
        match arg {
            ast::Arg::Named(named) => match named.name().as_str() {
                "id" => extract_id(&mut meta, named),
                "type" => extract_type(&mut meta, named),
                "created" => extract_created(&mut meta, named),
                "tags" => extract_tags(&mut meta, named),
                "origin" => extract_origin(&mut meta, named),
                unknown => meta.anomalies.push(MetaAnomaly::MalformedField(
                    unknown.to_string(),
                    named.expr().to_untyped().full_text().to_string(),
                )),
            },
            ast::Arg::Pos(expr) => {
                meta.anomalies.push(MetaAnomaly::MalformedField(
                    "<positional>".to_string(),
                    expr.to_untyped().full_text().to_string(),
                ))
            }
            ast::Arg::Spread(spread) => {
                meta.anomalies.push(MetaAnomaly::MalformedField(
                    "<spread>".to_string(),
                    spread.to_untyped().full_text().to_string(),
                ))
            }
        }
    }
    meta
}

fn extract_id(meta: &mut Meta, named: ast::Named) {
    match get_string_value(named.expr()) {
        Some(value) => meta.id = Some(NoteId(value)),
        None => meta.anomalies.push(MetaAnomaly::MalformedField(
            "id".to_string(),
            named.expr().to_untyped().full_text().to_string(),
        )),
    }
}

fn extract_type(meta: &mut Meta, named: ast::Named) {
    match get_string_value(named.expr()) {
        Some(value) => meta.note_type = Some(NoteType::from_name(&value)),
        None => meta.anomalies.push(MetaAnomaly::MalformedField(
            "type".to_string(),
            named.expr().to_untyped().full_text().to_string(),
        )),
    }
}

fn extract_created(meta: &mut Meta, named: ast::Named) {
    match get_string_value(named.expr()) {
        Some(raw) => match raw.parse::<Date>() {
            Ok(date) => meta.created = Some(date),
            Err(_) => meta.anomalies.push(MetaAnomaly::InvalidCreated(raw)),
        },
        None => meta.anomalies.push(MetaAnomaly::MalformedField(
            "created".to_string(),
            named.expr().to_untyped().full_text().to_string(),
        )),
    }
}

fn extract_tags(meta: &mut Meta, named: ast::Named) {
    if let ast::Expr::Array(array) = named.expr() {
        for item in array.items() {
            if let ast::ArrayItem::Pos(tag) = item {
                match get_string_value(tag) {
                    Some(value) => meta.tags.push(value),
                    None => meta.anomalies.push(MetaAnomaly::MalformedField(
                        "tags".to_string(),
                        tag.to_untyped().full_text().to_string(),
                    )),
                }
            } else {
                meta.anomalies.push(MetaAnomaly::MalformedField(
                    "tags".to_string(),
                    item.to_untyped().full_text().to_string(),
                ))
            }
        }
    } else {
        meta.anomalies.push(MetaAnomaly::MalformedField(
            "tags".to_string(),
            named.expr().to_untyped().full_text().to_string(),
        ))
    }
}

fn extract_origin(meta: &mut Meta, named: ast::Named) {
    match get_string_value(named.expr()) {
        Some(value) => meta.origin = Some(value),
        None => meta.anomalies.push(MetaAnomaly::MalformedField(
            "origin".to_string(),
            named.expr().to_untyped().full_text().to_string(),
        )),
    }
}

pub(crate) fn extract_link_target(call: ast::FuncCall) -> Option<NoteId> {
    for arg in call.args().items() {
        if let ast::Arg::Pos(expr) = arg {
            return get_string_value(expr).map(NoteId);
        }
    }
    None
}

fn get_string_value(expr: ast::Expr) -> Option<String> {
    if let ast::Expr::Str(s) = expr {
        return Some(s.get().to_string());
    }
    None
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn valid_created_and_origin_are_extracted() {
        let meta = present(parse_note(
            r#"#meta(created: "2026-07-21", origin: "smart-notes")"#,
        ));
        assert_eq!(meta.created, Some(jiff::civil::date(2026, 7, 21)));
        assert_eq!(meta.origin, Some("smart-notes".to_string()));
        assert_eq!(meta.anomalies, vec![]);
    }

    #[test]
    fn empty_file_yields_missing_meta_and_no_links() {
        let parsed = parse_note("");
        assert_eq!(parsed.meta, MetaStatus::Missing);
        assert_eq!(parsed.links, vec![]);
    }

    #[test]
    fn unknown_type_name_is_kept_as_data() {
        let meta = present(parse_note(r#"#meta(type: "concpet")"#));
        assert_eq!(
            meta.note_type,
            Some(NoteType::Unknown("concpet".to_string()))
        );
        assert_eq!(meta.anomalies, vec![]);
    }

    #[test]
    fn non_string_id_is_field_level_debt() {
        let meta = present(parse_note(r#"#meta(id: 42, type: "concept")"#));
        assert_eq!(meta.id, None);
        assert_eq!(meta.note_type, Some(NoteType::Concept));
        assert_eq!(meta.anomalies, vec![malformed("id", "42")]);
    }

    #[test]
    fn non_string_type_is_field_level_debt() {
        let meta = present(parse_note("#meta(type: 3)"));
        assert_eq!(meta.note_type, None);
        assert_eq!(meta.anomalies, vec![malformed("type", "3")]);
    }

    #[test]
    fn unparseable_created_keeps_other_fields() {
        let meta =
            present(parse_note(r#"#meta(id: "x", created: "july 21st")"#));
        assert_eq!(meta.id, Some(NoteId("x".to_string())));
        assert_eq!(meta.created, None);
        assert_eq!(
            meta.anomalies,
            vec![MetaAnomaly::InvalidCreated("july 21st".to_string())]
        );
    }

    #[test]
    fn non_string_created_is_malformed_not_invalid() {
        let meta = present(parse_note("#meta(created: 3)"));
        assert_eq!(meta.created, None);
        assert_eq!(meta.anomalies, vec![malformed("created", "3")]);
    }

    #[test]
    fn non_string_origin_is_field_level_debt() {
        let meta = present(parse_note("#meta(origin: 3)"));
        assert_eq!(meta.origin, None);
        assert_eq!(meta.anomalies, vec![malformed("origin", "3")]);
    }

    #[test]
    fn non_array_tags_are_malformed() {
        let meta = present(parse_note(r#"#meta(tags: "solo")"#));
        assert_eq!(meta.tags, Vec::<String>::new());
        assert_eq!(meta.anomalies, vec![malformed("tags", r#""solo""#)]);
    }

    #[test]
    fn parenthesized_tags_without_trailing_comma_are_malformed() {
        // ("x") is a parenthesized string in typst, not a one-element array;
        // vanilla typst would reject it too (template.typ calls tags.map).
        let meta = present(parse_note(r#"#meta(tags: ("x"))"#));
        assert_eq!(meta.tags, Vec::<String>::new());
        assert_eq!(meta.anomalies, vec![malformed("tags", r#"("x")"#)]);
    }

    #[test]
    fn bad_tag_element_is_debt_but_good_ones_are_kept() {
        let meta = present(parse_note(r#"#meta(tags: ("a", 3, "b"))"#));
        assert_eq!(meta.tags, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(meta.anomalies, vec![malformed("tags", "3")]);
    }

    #[test]
    fn spread_tag_element_is_debt() {
        let meta = present(parse_note(r#"#meta(tags: ("a", ..rest))"#));
        assert_eq!(meta.tags, vec!["a".to_string()]);
        assert_eq!(meta.anomalies, vec![malformed("tags", "..rest")]);
    }

    #[test]
    fn unknown_field_is_debt() {
        let meta = present(parse_note(r#"#meta(foo: "bar")"#));
        assert_eq!(meta.anomalies, vec![malformed("foo", r#""bar""#)]);
    }

    #[test]
    fn positional_and_spread_meta_args_are_debt() {
        let meta = present(parse_note(r#"#meta("oops", ..stuff)"#));
        assert_eq!(
            meta.anomalies,
            vec![
                malformed("<positional>", r#""oops""#),
                malformed("<spread>", "..stuff"),
            ]
        );
    }

    #[test]
    fn duplicate_meta_first_wins_and_is_flagged() {
        let meta = present(parse_note(
            r#"#meta(id: "premier") #meta(id: "second") #meta(id: "troisième")"#,
        ));
        assert_eq!(meta.id, Some(NoteId("premier".to_string())));
        assert_eq!(
            meta.anomalies,
            vec![MetaAnomaly::DuplicateMeta, MetaAnomaly::DuplicateMeta]
        );
    }

    #[test]
    fn link_without_string_argument_links_nowhere() {
        // first positional argument is the target; a non-string one is the
        // user's mistake, not something to fish a later string out of
        assert_eq!(parse_note("#l()").links, vec![]);
        assert_eq!(parse_note(r#"#l(3, "x")"#).links, vec![]);
    }

    #[test]
    fn link_with_only_named_arguments_links_nowhere() {
        assert_eq!(parse_note("#l(fill: red)").links, vec![]);
    }

    #[test]
    fn link_nested_in_other_markup_is_found() {
        let parsed = parse_note(r#"Voir #emph[#l("nested")] ici."#);
        assert_eq!(
            parsed.links,
            vec![Link {
                target: NoteId("nested".to_string())
            }]
        );
    }

    #[test]
    fn other_function_calls_are_ignored() {
        let parsed = parse_note(r#"#box(width: 1cm) #text(size: 8pt)[x]"#);
        assert_eq!(parsed.meta, MetaStatus::Missing);
        assert_eq!(parsed.links, vec![]);
    }

    #[test]
    fn get_string_value_rejects_non_string_expressions() {
        let root = typst_syntax::parse("#meta(id: (1, 2))");
        let mut stack = vec![&root];
        for _ in 0..MAX_NODES {
            let Some(node) = stack.pop() else { break };
            if let Some(call) = node.cast::<ast::FuncCall>() {
                for arg in call.args().items() {
                    if let ast::Arg::Named(named) = arg {
                        assert_eq!(get_string_value(named.expr()), None);
                        return;
                    }
                }
            }
            stack.extend(node.children().rev());
        }
        panic!("no named argument found in the probe source");
    }

    #[test]
    fn the_first_level_one_heading_is_the_title() {
        let parsed = parse_note("= Atomic notes\n\nprose\n\n= Second\n");
        assert_eq!(parsed.title, Some("Atomic notes".to_string()));
    }

    #[test]
    fn deeper_headings_are_sections_not_titles() {
        let parsed = parse_note("== A section\n\n=== Another\n");
        assert_eq!(parsed.title, None);
        // a section above the title does not shadow it
        let parsed = parse_note("== A section\n\n= The title\n");
        assert_eq!(parsed.title, Some("The title".to_string()));
    }

    #[test]
    fn a_note_without_a_heading_has_no_title() {
        assert_eq!(parse_note("").title, None);
        assert_eq!(parse_note("just prose\n").title, None);
    }

    #[test]
    fn a_blank_heading_names_nothing() {
        assert_eq!(parse_note("=\n").title, None);
        assert_eq!(parse_note("=   \n").title, None);
    }

    #[test]
    fn a_title_keeps_the_markup_it_contains_verbatim() {
        // the title is a label, not rendered content: what the picker
        // searches is the source text as written
        let parsed = parse_note(r#"= Notes on #emph[flow]"#);
        assert_eq!(parsed.title, Some("Notes on #emph[flow]".to_string()));
    }

    fn present(parsed: ParsedNote) -> Meta {
        match parsed.meta {
            MetaStatus::Present(meta) => meta,
            MetaStatus::Missing => panic!("expected a #meta call, found none"),
        }
    }

    fn malformed(field: &str, raw: &str) -> MetaAnomaly {
        MetaAnomaly::MalformedField(field.to_string(), raw.to_string())
    }
}
