//! Pure logic behind the open-loops list: the three kinds of debt, read as
//! one flat list of lines. Everything decidable without a VirtualDom lives
//! here, so the components stay wiring (`adr/2026-07-ui-covered-at-100.md`).

use std::path::PathBuf;

use crate::domain::stem_of;
use crate::index::DanglingLink;
use crate::logs::STILL_OPEN;

/// Every open loop, one line each, in query order: typeless notes, then
/// dangling links, then captures still owing their summary
/// (`adr/2026-08-loops-list-overlay.md`). The count in the chrome is this
/// list's length, so the ember and the list cannot disagree.
///
/// Notes are named by their stem, which is their id — the label the rest of
/// the app shows them under. The tag after the `·` says which loop it is,
/// and that is the whole vocabulary: no ages, no grouping, no actions
/// (`adr/2026-07-debt-counter-then-list.md`).
pub fn lines(
    typeless: &[PathBuf],
    dangling: &[DanglingLink],
    unsummarized: &[PathBuf],
) -> Vec<String> {
    let typeless = typeless
        .iter()
        .map(|path| format!("{} · typeless", stem_of(path)));
    let dangling = dangling.iter().map(|link| {
        format!("{} → {} · dangling", stem_of(&link.source), link.target.0)
    });
    let unsummarized = unsummarized
        .iter()
        .map(|path| format!("{} · {STILL_OPEN}", stem_of(path)));
    typeless.chain(dangling).chain(unsummarized).collect()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::domain::NoteId;

    fn dangling(source: &str, target: &str) -> DanglingLink {
        DanglingLink {
            source: PathBuf::from(source),
            target: NoteId(target.to_string()),
        }
    }

    #[test]
    fn each_kind_of_debt_says_what_it_is() {
        assert_eq!(
            lines(
                &[PathBuf::from("permanent/mystere.typ")],
                &[dangling("time/2026-07-22.typ", "fantome")],
                &[PathBuf::from("capture/capture-articles-zettel.typ")],
            ),
            vec![
                "mystere · typeless".to_string(),
                "2026-07-22 → fantome · dangling".to_string(),
                "capture-articles-zettel · still open".to_string(),
            ]
        );
    }

    #[test]
    fn a_vault_with_nothing_open_lists_nothing() {
        assert_eq!(lines(&[], &[], &[]), Vec::<String>::new());
    }

    #[test]
    fn every_item_of_every_kind_gets_its_own_line() {
        let list = lines(
            &[PathBuf::from("a.typ"), PathBuf::from("b.typ")],
            &[dangling("c.typ", "x"), dangling("c.typ", "y")],
            &[PathBuf::from("d.typ")],
        );
        assert_eq!(list.len(), 5, "the count is the list: {list:?}");
    }
}
