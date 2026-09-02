//! Pure logic behind the open-loops list: the three kinds of debt, read as
//! one flat list of lines. Everything decidable without a VirtualDom lives
//! here, so the components stay wiring (`adr/2026-07-ui-covered-at-100.md`).

use std::path::PathBuf;

use jiff::civil::Date;

use crate::domain::stem_of;
use crate::index::{DanglingLink, DueNote};
use crate::logs::STILL_OPEN;

/// One line in the open-loops overlay: what it says, and the path of the
/// note that OWES the debt — for a dangling link that is the link's
/// source, not its missing target. Clicking or Enter-ing the line opens
/// this note directly by path, never through the index's id lookup: a
/// note that owes debt because its own `#meta` is missing or broken has no
/// id row `path_for_id` could ever resolve, and the path is the one thing
/// every family already carries (`adr/2026-09-loop-lines-open-their-notes.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopLine {
    pub text: String,
    pub path: PathBuf,
}

/// Every open loop, one line each, in query order: typeless notes, then
/// dangling links, then captures still owing their summary, then the
/// notes the index could not read cleanly, then the notes whose `due` day
/// has come or gone
/// (`adr/2026-08-loops-list-overlay.md`,
/// `adr/2026-08-anomalies-join-the-loops.md`,
/// `adr/2026-09-course-type-and-due-loops.md`). The count in the chrome is
/// this list's length, so the ember and the list cannot disagree.
///
/// Notes are named by their stem, which is their id — the label the rest of
/// the app shows them under. The tag after the `·` says which loop it is,
/// no grouping, no actions (`adr/2026-07-debt-counter-then-list.md`); the
/// due families are the one place a date is spoken, because the date is
/// the debt.
pub fn lines(
    typeless: &[PathBuf],
    dangling: &[DanglingLink],
    unsummarized: &[PathBuf],
    anomalous: &[(PathBuf, String)],
    due: &[DueNote],
    today: Date,
) -> Vec<LoopLine> {
    let typeless = typeless.iter().map(|path| {
        let id = stem_of(path);
        LoopLine {
            text: format!("{id} · typeless"),
            path: path.clone(),
        }
    });
    let dangling = dangling.iter().map(|link| {
        let id = stem_of(&link.source);
        LoopLine {
            text: format!("{id} → {} · dangling", link.target.0),
            path: link.source.clone(),
        }
    });
    let unsummarized = unsummarized.iter().map(|path| {
        let id = stem_of(path);
        LoopLine {
            text: format!("{id} · {STILL_OPEN}"),
            path: path.clone(),
        }
    });
    let anomalous = anomalous.iter().map(|(path, family)| {
        let id = stem_of(path);
        LoopLine {
            text: format!("{id} · {family}"),
            path: path.clone(),
        }
    });
    let due = due.iter().map(|note| {
        let id = stem_of(&note.path);
        let text = if note.due < today {
            format!("{id} · overdue since {}", note.due)
        } else {
            format!("{id} · due {}", note.due)
        };
        LoopLine {
            text,
            path: note.path.clone(),
        }
    });
    typeless
        .chain(dangling)
        .chain(unsummarized)
        .chain(anomalous)
        .chain(due)
        .collect()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::domain::NoteId;

    const TODAY: Date = jiff::civil::date(2026, 7, 24);

    fn dangling(source: &str, target: &str) -> DanglingLink {
        DanglingLink {
            source: PathBuf::from(source),
            target: NoteId(target.to_string()),
        }
    }

    fn line(text: &str, path: &str) -> LoopLine {
        LoopLine {
            text: text.to_string(),
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn each_kind_of_debt_says_what_it_is() {
        assert_eq!(
            lines(
                &[PathBuf::from("permanent/mystere.typ")],
                &[dangling("time/2026-07-22.typ", "fantome")],
                &[PathBuf::from("capture/capture-articles-zettel.typ")],
                &[
                    (
                        PathBuf::from("permanent/bancal.typ"),
                        "malformed meta".to_string(),
                    ),
                    (
                        PathBuf::from("permanent/fleuve.typ"),
                        "truncated".to_string(),
                    ),
                ],
                &[
                    DueNote {
                        path: PathBuf::from("permanent/devoir-1.typ"),
                        due: jiff::civil::date(2026, 7, 20),
                    },
                    DueNote {
                        path: PathBuf::from("permanent/examen.typ"),
                        due: TODAY,
                    },
                ],
                TODAY,
            ),
            vec![
                line("mystere · typeless", "permanent/mystere.typ"),
                line("2026-07-22 → fantome · dangling", "time/2026-07-22.typ"),
                line(
                    "capture-articles-zettel · still open",
                    "capture/capture-articles-zettel.typ"
                ),
                line("bancal · malformed meta", "permanent/bancal.typ"),
                line("fleuve · truncated", "permanent/fleuve.typ"),
                line(
                    "devoir-1 · overdue since 2026-07-20",
                    "permanent/devoir-1.typ"
                ),
                line("examen · due 2026-07-24", "permanent/examen.typ"),
            ]
        );
    }

    #[test]
    fn a_dangling_links_path_is_the_source_not_the_missing_target() {
        let list = lines(
            &[],
            &[dangling("time/2026-07-22.typ", "fantome")],
            &[],
            &[],
            &[],
            TODAY,
        );
        assert_eq!(
            list,
            vec![line(
                "2026-07-22 → fantome · dangling",
                "time/2026-07-22.typ"
            )],
            "the path is the note that owes the link, not the id it names"
        );
    }

    #[test]
    fn a_vault_with_nothing_open_lists_nothing() {
        assert_eq!(
            lines(&[], &[], &[], &[], &[], TODAY),
            Vec::<LoopLine>::new()
        );
    }

    #[test]
    fn every_item_of_every_kind_gets_its_own_line() {
        let list = lines(
            &[PathBuf::from("a.typ"), PathBuf::from("b.typ")],
            &[dangling("c.typ", "x"), dangling("c.typ", "y")],
            &[PathBuf::from("d.typ")],
            &[(PathBuf::from("e.typ"), "malformed meta".to_string())],
            &[DueNote {
                path: PathBuf::from("f.typ"),
                due: TODAY,
            }],
            TODAY,
        );
        assert_eq!(list.len(), 7, "the count is the list: {list:?}");
    }
}
