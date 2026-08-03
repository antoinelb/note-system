//! Pure logic behind the links UX: what the Ctrl+L picker matches, and what
//! the footer under an open note says in each direction. Everything decidable
//! without a VirtualDom lives here, so the components stay wiring
//! (`adr/2026-07-ui-covered-at-100.md`).

use std::path::Path;

use crate::domain::{NoteId, NoteType};
use crate::index::Backlink;

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
/// and its tests cannot disagree about the link's shape.
pub fn format_link(id: &str) -> String {
    format!("#l(\"{id}\")")
}

/// One footer entry, either direction.
///
/// `scale: Some` = a time note the logs centre pane can open, so the entry is
/// clickable; `None` = permanent, capture or generated — visible but inert
/// until v1's table has somewhere to show it
/// (`adr/2026-07-permanent-notes-wait-for-table.md`).
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
                None => stem_of(&row.source),
            };
            (label != own).then(|| FooterLink {
                scale: scale_of(&label, time_notes),
                label,
                dangling: false,
            })
        })
        .collect()
}

/// The scale a time note is filed under, or `None` for everything the logs
/// screen cannot display.
fn scale_of(id: &str, time_notes: &[(String, NoteType)]) -> Option<NoteType> {
    time_notes
        .iter()
        .find(|(known, _)| known == id)
        .map(|(_, scale)| scale.clone())
}

/// A path's filename without its extension; a path that ends in no name at
/// all keeps whatever it does have, so the entry is never blank.
fn stem_of(path: &Path) -> String {
    path.file_stem()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
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
    fn a_completion_writes_a_plain_l_call() {
        assert_eq!(format_link("atomic-notes"), "#l(\"atomic-notes\")");
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
