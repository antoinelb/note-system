//! Pure logic behind the Ctrl+N create overlay: the type vocabulary the
//! first step offers and the creation the second step commits. Everything
//! decidable without a VirtualDom lives here, so the component stays wiring
//! (adr/2026-07-ui-covered-at-100.md, adr/2026-08-ctrl-n-two-step-create-overlay.md).

use std::path::{Path, PathBuf};

use crate::domain::{NoteCategory, NoteType, stem_of};
use crate::template::{self, TemplateError};

/// The nine permanent types, in the order the picker lists them — the
/// wireframe palette's order plus the two turn-1 additions, the same order
/// the type bars are documented in. A course is a `project`
/// (adr/2026-09-a-course-is-a-project.md); `tool` is anything that helps
/// do work and comes last, the one type added after the set closed
/// (adr/2026-09-tool-is-the-ninth-permanent-type.md).
pub const TYPES: [NoteType; 9] = [
    NoteType::Person,
    NoteType::Organisation,
    NoteType::Source,
    NoteType::Concept,
    NoteType::Claim,
    NoteType::Idea,
    NoteType::Personal,
    NoteType::Project,
    NoteType::Tool,
];

/// The types a query leaves — the palette's contains rule, over a closed
/// vocabulary an empty query shows whole.
pub fn filter(query: &str) -> Vec<NoteType> {
    let needle = query.to_lowercase();
    TYPES
        .iter()
        .filter(|note_type| note_type.as_name().contains(&needle))
        .cloned()
        .collect()
}

/// The overlay's Enter: a permanent note from its type's template. Answers
/// the new id alongside the path — the caller places the card and opens the
/// sheet by id, and the id (the kebab'd title) is `create`'s to decide.
pub fn permanent(
    vault: &Path,
    note_type: &NoteType,
    title: &str,
    created: &str,
) -> Result<(String, PathBuf), TemplateError> {
    let path = template::create(
        vault,
        &NoteCategory::Permanent,
        note_type,
        title,
        created,
        "",
    )?;
    Ok((stem_of(&path), path))
}

/// The overlay's notice line for a refused creation: the two amendable
/// refusals in words aimed at the title field, everything else verbatim —
/// an unreadable template is not the title's fault.
pub fn notice(error: &TemplateError) -> String {
    match error {
        TemplateError::AlreadyExists(_) => {
            "a note with this id already exists".to_string()
        }
        TemplateError::EmptyId(_) => {
            "the title needs at least one letter or digit".to_string()
        }
        other => format!("create: {other:?}"),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn the_nine_types_stand_in_picker_order() {
        let names: Vec<&str> = TYPES.iter().map(NoteType::as_name).collect();
        assert_eq!(
            names,
            vec![
                "person",
                "organisation",
                "source",
                "concept",
                "claim",
                "idea",
                "personal",
                "project",
                "tool"
            ]
        );
    }

    #[test]
    fn the_query_narrows_types_ignoring_case() {
        assert_eq!(filter("PER"), vec![NoteType::Person, NoteType::Personal]);
        assert_eq!(filter("concept"), vec![NoteType::Concept]);
        assert_eq!(filter(""), TYPES.to_vec());
        assert_eq!(filter("xyzzy"), Vec::<NoteType>::new());
    }

    fn vault_with_concept() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        for sub in ["templates", "permanent"] {
            std::fs::create_dir(dir.path().join(sub))
                .expect("create vault dir");
        }
        std::fs::write(
            dir.path().join("templates/concept.typ"),
            "= {{title}} ({{id}}, {{created}})\n",
        )
        .expect("write template");
        dir
    }

    #[test]
    fn permanent_answers_the_new_id_and_path() {
        let dir = vault_with_concept();
        let (id, path) = permanent(
            dir.path(),
            &NoteType::Concept,
            "Deep Modules",
            "2026-08-09",
        )
        .expect("create");
        assert_eq!(id, "deep-modules");
        assert_eq!(path, dir.path().join("permanent/deep-modules.typ"));
        assert!(path.exists());
    }

    #[test]
    fn template_errors_pass_through() {
        let dir = vault_with_concept();
        let collided =
            permanent(dir.path(), &NoteType::Concept, "Note", "2026-08-09");
        assert!(collided.is_ok());
        let again =
            permanent(dir.path(), &NoteType::Concept, "Note", "2026-08-09");
        assert!(matches!(again, Err(TemplateError::AlreadyExists(_))));
        let hopeless =
            permanent(dir.path(), &NoteType::Concept, "???", "2026-08-09");
        assert!(matches!(hopeless, Err(TemplateError::EmptyId(_))));
    }

    #[test]
    fn the_notice_speaks_to_the_title_field() {
        assert_eq!(
            notice(&TemplateError::AlreadyExists("x".into())),
            "a note with this id already exists"
        );
        assert_eq!(
            notice(&TemplateError::EmptyId("???".to_string())),
            "the title needs at least one letter or digit"
        );
        assert_eq!(
            notice(&TemplateError::UnknownTemplate("concept".to_string())),
            "create: UnknownTemplate(\"concept\")"
        );
    }
}
