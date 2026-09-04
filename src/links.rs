//! Pure logic behind the links UX: what the Ctrl+L picker matches, and what
//! the footer under an open note says in each direction. Everything decidable
//! without a VirtualDom lives here, so the components stay wiring
//! (`adr/2026-07-ui-covered-at-100.md`).

use typst_syntax::ast;

use crate::domain::{NoteId, NoteType};
use crate::index::Backlink;
use crate::parse::MAX_NODES;

/// The picker never shows more than this many rows: past a handful, reading
/// the list costs more than typing one more letter.
pub const MAX_MATCHES: usize = 8;

/// One linkable note as the picker knows it — the id it will write, and the
/// title it can also be found by (`adr/2026-08-titles-in-index.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub id: String,
    pub title: Option<String>,
}

impl Completion {
    pub fn new((id, title): (String, Option<String>)) -> Completion {
        Completion { id, title }
    }
}

/// The picker's matches for `query`: a case-insensitive substring of either
/// the id or the title, in the index's order (by id), capped. An empty query
/// is the whole list — opening the picker shows what is there.
pub fn filter<'a>(
    entries: &'a [Completion],
    query: &str,
) -> Vec<&'a Completion> {
    let needle = query.to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            entry.id.to_lowercase().contains(&needle)
                || entry
                    .title
                    .as_ref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle))
        })
        .take(MAX_MATCHES)
        .collect()
}

/// The text a completion writes into the buffer. One source, so the picker
/// and its tests cannot disagree about the link's shape
/// (adr/2026-09-wiki-links-replace-the-l-call.md).
pub fn format_link(id: &str) -> String {
    format!("[[{id}]]")
}

/// Whether the caret at `head` in `block_text` sits inside the empty
/// `[[]]` the autopairs leave behind a second typed `[` — the moment the
/// picker is summoned by typing rather than by Ctrl+L.
pub fn typed_wiki_opening(block_text: &str, head: usize) -> bool {
    block_text
        .get(..head)
        .is_some_and(|before| before.ends_with("[["))
        && block_text
            .get(head..)
            .is_some_and(|after| after.starts_with("]]"))
}

/// One footer entry, either direction.
///
/// `scale: Some` = a time note the logs centre pane can open; `None` =
/// permanent, capture or generated, which open their card's writing sheet
/// on the table (`adr/2026-08-permanent-links-open-sheets.md`). Only a
/// dangling link — or a backlink labelled by the stem of an id-less note,
/// which no card can host — stays inert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterLink {
    pub label: String,
    pub scale: Option<NoteType>,
    pub dangling: bool,
}

/// "→": the open buffer's link targets, classified. Read from the live text
/// rather than the index, so a link marks itself dangling as it is typed
/// (`adr/2026-08-links-footer-both-directions.md`). `resolves` answers
/// whether a target exists at all; `own` is the open note's id, because a
/// note linking to itself is not news.
pub fn outgoing(
    targets: &[NoteId],
    own: &str,
    resolves: impl Fn(&str) -> bool,
    time_notes: &[(String, NoteType)],
) -> Vec<FooterLink> {
    let mut seen = Vec::new();
    for target in targets {
        let id = target.0.as_str();
        if id == own || seen.iter().any(|link: &FooterLink| link.label == id) {
            continue;
        }
        let exists = resolves(id);
        seen.push(FooterLink {
            label: id.to_string(),
            scale: exists.then(|| scale_of(id, time_notes)).flatten(),
            dangling: !exists,
        });
    }
    seen
}

/// "←": the notes linking here, labelled by their own id — or by their
/// filename when they have none, which is open-loops debt of its own but
/// still a real link. A backlink's source exists by construction, so nothing
/// in this direction can dangle.
pub fn backlinks(
    rows: &[Backlink],
    own: &str,
    time_notes: &[(String, NoteType)],
) -> Vec<FooterLink> {
    rows.iter()
        .filter_map(|row| {
            let label = match &row.id {
                Some(id) => id.clone(),
                None => crate::domain::stem_of(&row.source),
            };
            (label != own).then(|| FooterLink {
                scale: scale_of(&label, time_notes),
                label,
                dangling: false,
            })
        })
        .collect()
}

/// What a link under the caret leads to: a note, by the id a `[[id]]`
/// spells, or a resource — a file under the vault, a URL — by the
/// destination of Typst's own `#link`, which the desktop's launcher opens
/// and the index never counts as debt
/// (`adr/2026-09-link-is-for-resources.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    Note(String),
    Resource(String),
}

/// Where a `#link` destination points once the vault is known: a leading
/// `/` is the vault root, as it is for Typst's own path resolution
/// (`#import "/templates/…"`), and anything else — a URL, a relative
/// path — is handed to the launcher as written.
pub fn resolve_resource(root: &std::path::Path, destination: &str) -> String {
    match destination.strip_prefix('/') {
        Some(inside) => root.join(inside).display().to_string(),
        None => destination.to_string(),
    }
}

/// The `[[...]]` or `#link("...")` target the caret is standing in, for
/// Ctrl+Enter (`adr/2026-08-ctrl-enter-opens-time-links.md`). `caret` is a
/// byte offset into `block_text` — the active block's own source, the same
/// slice the caret probe measures. A `#link` with no string target leads
/// nowhere, exactly as the footer reads it.
pub fn link_at(block_text: &str, caret: usize) -> Option<LinkTarget> {
    let root = typst_syntax::parse(block_text);
    // typst tokenizes the `#` as the call's left sibling rather than part of
    // it, so the caret sitting on it is outside the span by a byte. Probing
    // one further lets the chord fire from the very start of a `#link`,
    // where Home leaves the caret on a line that opens with one — only
    // from a `#`, so the byte before a `[[` stays outside it.
    target_at(&root, caret).or_else(|| {
        (block_text.as_bytes().get(caret) == Some(&b'#'))
            .then(|| target_at(&root, caret + 1))
            .flatten()
    })
}

/// The link whose span holds `caret`, walked from `root`. Both edges count
/// as inside, so the caret just past a `]]` or a `)` still follows that
/// link.
fn target_at(
    root: &typst_syntax::SyntaxNode,
    caret: usize,
) -> Option<LinkTarget> {
    // (start, node); only nodes whose span holds the caret are ever pushed
    // back, so the walk descends one root-to-link path rather than the tree
    let mut stack = vec![(0usize, root)];

    for _ in 0..MAX_NODES {
        let Some((start, node)) = stack.pop() else {
            break;
        };
        if caret < start || caret > start + node.len() {
            continue;
        }
        if let Some(id) = wiki_link_holding(node, start, caret) {
            return Some(LinkTarget::Note(id));
        }
        if let Some(call) = node.cast::<ast::FuncCall>()
            && let ast::Expr::Ident(name) = call.callee()
        {
            if name.as_str() != "link" {
                push_children(&mut stack, start, node);
                continue;
            }
            return crate::parse::extract_link_target(call)
                .map(|id| LinkTarget::Resource(id.0));
        }
        push_children(&mut stack, start, node);
    }
    None
}

/// The `[[id]]` among `node`'s children whose bytes hold `caret`, both
/// edges included; two links touching at the caret resolve to the first,
/// the way reading does.
fn wiki_link_holding(
    node: &typst_syntax::SyntaxNode,
    start: usize,
    caret: usize,
) -> Option<String> {
    let offsets: Vec<usize> = node
        .children()
        .scan(start, |offset, child| {
            let at = *offset;
            *offset += child.len();
            Some(at)
        })
        .chain(std::iter::once(start + node.len()))
        .collect();
    crate::parse::wiki_links(node)
        .into_iter()
        .find(|link| {
            let first = offsets.get(link.children.start).copied();
            let past = offsets.get(link.children.end).copied();
            first
                .zip(past)
                .is_some_and(|(first, past)| (first..=past).contains(&caret))
        })
        .map(|link| link.id)
}

/// The node's children with their own offsets, ordered so popping walks the
/// text left to right.
fn push_children<'a>(
    stack: &mut Vec<(usize, &'a typst_syntax::SyntaxNode)>,
    start: usize,
    node: &'a typst_syntax::SyntaxNode,
) {
    let base = stack.len();
    let mut offset = start;
    for child in node.children() {
        stack.push((offset, child));
        offset += child.len();
    }
    stack[base..].reverse();
}

/// The scale a time note is filed under, or `None` for everything the logs
/// screen cannot display.
pub fn scale_of(
    id: &str,
    time_notes: &[(String, NoteType)],
) -> Option<NoteType> {
    time_notes
        .iter()
        .find(|(known, _)| known == id)
        .map(|(_, scale)| scale.clone())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn completions() -> Vec<Completion> {
        [
            ("atomic-notes", Some("Atomic notes recombine better")),
            ("capture-idea", None),
            ("luhmann", Some("Niklas Luhmann")),
            ("zettelkasten", Some("Zettelkasten")),
        ]
        .into_iter()
        .map(|(id, title)| Completion {
            id: id.to_string(),
            title: title.map(str::to_string),
        })
        .collect()
    }

    fn ids(matches: Vec<&Completion>) -> Vec<String> {
        matches.into_iter().map(|entry| entry.id.clone()).collect()
    }

    #[test]
    fn the_query_matches_ids_and_titles_alike() {
        let all = completions();
        assert_eq!(ids(filter(&all, "atomic")), vec!["atomic-notes"]);
        // "Niklas" appears only in the title
        assert_eq!(ids(filter(&all, "niklas")), vec!["luhmann"]);
        // and a note with no title is still found by its id
        assert_eq!(ids(filter(&all, "idea")), vec!["capture-idea"]);
    }

    #[test]
    fn matching_ignores_case_in_both_directions() {
        let all = completions();
        assert_eq!(ids(filter(&all, "ZETTEL")), vec!["zettelkasten"]);
        assert_eq!(ids(filter(&all, "luHMann")), vec!["luhmann"]);
    }

    #[test]
    fn an_empty_query_offers_everything_and_nothing_matches_nonsense() {
        let all = completions();
        assert_eq!(filter(&all, "").len(), 4);
        assert_eq!(filter(&all, "xyzzy"), Vec::<&Completion>::new());
    }

    #[test]
    fn the_list_is_capped_so_it_stays_readable() {
        let many: Vec<Completion> = (0..MAX_MATCHES + 5)
            .map(|n| Completion {
                id: format!("note-{n}"),
                title: None,
            })
            .collect();
        assert_eq!(filter(&many, "note").len(), MAX_MATCHES);
    }

    #[test]
    fn a_completion_writes_a_wiki_link() {
        assert_eq!(format_link("atomic-notes"), "[[atomic-notes]]");
    }

    #[test]
    fn the_typed_opening_is_the_empty_pair_around_the_caret() {
        assert!(typed_wiki_opening("Voir [[]]", 7));
        assert!(!typed_wiki_opening("Voir [[]]", 6));
        assert!(!typed_wiki_opening("Voir [[x]]", 7));
        assert!(!typed_wiki_opening("Voir [[]", 7));
        assert!(!typed_wiki_opening("[]", 1));
    }

    #[test]
    fn the_caret_finds_the_link_it_stands_in() {
        // the link spans bytes 5..16
        let text = "Voir [[luhmann]] demain";
        assert_eq!(link_at(text, 9), Some(note("luhmann")));
        // from the first '[' and from just past the last ']'
        assert_eq!(link_at(text, 5), Some(note("luhmann")));
        assert_eq!(link_at(text, 16), Some(note("luhmann")));
        // a block that is nothing but a link, caret at its very start
        assert_eq!(link_at("[[seul]]", 0), Some(note("seul")));
    }

    #[test]
    fn a_caret_outside_every_link_finds_nothing() {
        let text = "Voir [[luhmann]] demain";
        assert_eq!(link_at(text, 4), None);
        assert_eq!(link_at(text, 17), None);
        assert_eq!(link_at("prose sans lien", 3), None);
        assert_eq!(link_at("", 0), None);
        assert_eq!(link_at("[[]]", 2), None);
    }

    #[test]
    fn the_caret_picks_the_link_it_is_in_not_its_neighbour() {
        let text = "[[a]] et [[bb]]";
        assert_eq!(link_at(text, 3), Some(note("a")));
        assert_eq!(link_at(text, 12), Some(note("bb")));
        // touching links resolve left to right, the way reading does
        assert_eq!(link_at("[[a]][[bb]]", 5), Some(note("a")));
    }

    fn note(id: &str) -> LinkTarget {
        LinkTarget::Note(id.to_string())
    }

    #[test]
    fn a_typst_link_is_a_resource_and_a_bare_call_between_is_walked_past() {
        // the destination, not the body, is what the caret follows
        let text = r#"see #link("https://example.org/x.pdf")[the slides]"#;
        assert_eq!(
            link_at(text, 12),
            Some(LinkTarget::Resource(
                "https://example.org/x.pdf".to_string()
            ))
        );
        // a call that is not `link` is descended, not answered:
        // the link nested inside it is still found
        assert_eq!(
            link_at(r#"#emph[#link("/assets/a.pdf")[a]]"#, 14),
            Some(LinkTarget::Resource("/assets/a.pdf".to_string()))
        );
        // a label destination is no string: nowhere; so is no destination
        assert_eq!(link_at("#link(<intro>)[x]", 8), None);
        assert_eq!(link_at("#link(fill: red)", 8), None);
        // from the `#` the call opens with, which typst keeps outside it
        assert_eq!(
            link_at(text, 4),
            Some(LinkTarget::Resource(
                "https://example.org/x.pdf".to_string()
            ))
        );
        assert_eq!(link_at("#emph[x]", 0), None);
    }

    #[test]
    fn a_resource_under_the_vault_resolves_against_the_root() {
        let root = std::path::Path::new("/vault");
        assert_eq!(
            resolve_resource(root, "/assets/slides.pdf"),
            "/vault/assets/slides.pdf"
        );
        assert_eq!(
            resolve_resource(root, "https://example.org"),
            "https://example.org"
        );
        assert_eq!(resolve_resource(root, "beside.pdf"), "beside.pdf");
    }

    #[test]
    fn a_link_nested_in_markup_is_still_under_the_caret() {
        assert_eq!(link_at("Voir *[[nested]]* ici", 9), Some(note("nested")));
    }

    #[test]
    fn a_caret_past_multibyte_text_still_lands_in_the_link() {
        // "été" is 5 bytes, so the byte offset is not the character count
        let text = "été [[hiver]]";
        assert_eq!(link_at(text, 9), Some(note("hiver")));
    }

    fn time_notes() -> Vec<(String, NoteType)> {
        vec![
            ("2026-07-22".to_string(), NoteType::Daily),
            ("2026-w30".to_string(), NoteType::Weekly),
        ]
    }

    fn targets(ids: &[&str]) -> Vec<NoteId> {
        ids.iter().map(|id| NoteId(id.to_string())).collect()
    }

    #[test]
    fn outgoing_links_are_clickable_inert_or_dangling() {
        let known = ["2026-07-22", "2026-w30", "luhmann"];
        let links = outgoing(
            &targets(&["2026-07-22", "luhmann", "fantome"]),
            "2026-07-23",
            |id| known.contains(&id),
            &time_notes(),
        );
        assert_eq!(
            links,
            vec![
                FooterLink {
                    label: "2026-07-22".to_string(),
                    scale: Some(NoteType::Daily),
                    dangling: false,
                },
                FooterLink {
                    label: "luhmann".to_string(),
                    scale: None,
                    dangling: false,
                },
                FooterLink {
                    label: "fantome".to_string(),
                    scale: None,
                    dangling: true,
                },
            ]
        );
    }

    #[test]
    fn outgoing_drops_self_links_and_repeats() {
        let links = outgoing(
            &targets(&["2026-07-23", "2026-w30", "2026-w30"]),
            "2026-07-23",
            |_| true,
            &time_notes(),
        );
        assert_eq!(
            links.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(),
            vec!["2026-w30"],
            "a note linking to itself says nothing, and twice is once"
        );
    }

    #[test]
    fn backlinks_are_labelled_by_id_or_by_filename() {
        let rows = vec![
            Backlink {
                source: PathBuf::from("time/2026-07-22.typ"),
                id: Some("2026-07-22".to_string()),
            },
            Backlink {
                source: PathBuf::from("permanent/luhmann.typ"),
                id: Some("luhmann".to_string()),
            },
            Backlink {
                source: PathBuf::from("permanent/anonyme.typ"),
                id: None,
            },
        ];
        assert_eq!(
            backlinks(&rows, "2026-07-23", &time_notes()),
            vec![
                FooterLink {
                    label: "2026-07-22".to_string(),
                    scale: Some(NoteType::Daily),
                    dangling: false,
                },
                FooterLink {
                    label: "luhmann".to_string(),
                    scale: None,
                    dangling: false,
                },
                FooterLink {
                    label: "anonyme".to_string(),
                    scale: None,
                    dangling: false,
                },
            ]
        );
    }

    #[test]
    fn a_note_linking_to_itself_is_not_its_own_backlink() {
        let rows = vec![Backlink {
            source: PathBuf::from("time/2026-07-23.typ"),
            id: Some("2026-07-23".to_string()),
        }];
        assert_eq!(
            backlinks(&rows, "2026-07-23", &time_notes()),
            Vec::<FooterLink>::new()
        );
    }

    #[test]
    fn a_source_path_with_no_filename_still_gets_a_label() {
        let rows = vec![Backlink {
            source: PathBuf::from(".."),
            id: None,
        }];
        assert_eq!(
            backlinks(&rows, "x", &time_notes())[0].label,
            "..".to_string()
        );
    }

    #[test]
    fn a_completion_is_built_from_the_index_row() {
        assert_eq!(
            Completion::new(("x".to_string(), None)),
            Completion {
                id: "x".to_string(),
                title: None,
            }
        );
    }
}
