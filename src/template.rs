use std::path::{Path, PathBuf};

use crate::domain::{NoteCategory, NoteType};

#[derive(Debug)]
pub enum TemplateError {
    UnknownTemplate(String),
    UnknownPlaceholder(String),
    EmptyId(String),
    AlreadyExists(PathBuf),
    Io(PathBuf, std::io::Error),
}

pub fn create(
    vault: &Path,
    category: &NoteCategory,
    note_type: &NoteType,
    title: &str,
    created: &str,
    content: &str,
) -> Result<PathBuf, TemplateError> {
    let id = kebab_id(title);
    if id.is_empty() {
        return Err(TemplateError::EmptyId(title.to_string()));
    }
    let template = read_template(vault, note_type)?;
    write_note(vault, category, &template, &id, title, created, content)
}

fn kebab_id(title: &str) -> String {
    let mut id = String::with_capacity(title.len());
    for char in title.to_lowercase().chars() {
        if char.is_alphanumeric() {
            id.push(char);
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    id.trim_end_matches('-').to_string()
}

fn read_template(
    vault: &Path,
    note_type: &NoteType,
) -> Result<String, TemplateError> {
    let name = note_type.as_name();
    let path = vault.join("templates").join(format!("{name}.typ"));
    std::fs::read_to_string(&path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            TemplateError::UnknownTemplate(name.to_string())
        }
        _ => TemplateError::Io(path, err),
    })
}

fn write_note(
    vault: &Path,
    category: &NoteCategory,
    template: &str,
    id: &str,
    title: &str,
    created: &str,
    content: &str,
) -> Result<PathBuf, TemplateError> {
    let path = vault.join(category.as_dir()).join(format!("{id}.typ"));
    if path.exists() {
        return Err(TemplateError::AlreadyExists(path));
    }
    let filled = fill(
        template,
        &[
            ("id", id),
            ("created", created),
            ("title", title),
            ("content", content),
        ],
    )?;
    std::fs::write(&path, &filled)
        .map_err(|err| TemplateError::Io(path.clone(), err))?;
    Ok(path)
}

fn fill(
    template: &str,
    values: &[(&str, &str)],
) -> Result<String, TemplateError> {
    let mut segments = template.split("{{");
    let mut filled = segments.next().unwrap_or("").to_string();
    for segment in segments {
        match segment.split_once("}}") {
            Some((name, rest)) => {
                let Some((_, val)) =
                    values.iter().find(|(key, _)| *key == name)
                else {
                    return Err(TemplateError::UnknownPlaceholder(
                        name.to_string(),
                    ));
                };
                filled.push_str(val);
                filled.push_str(rest);
            }
            None => {
                filled.push_str("{{");
                filled.push_str(segment);
            }
        }
    }
    Ok(filled)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    const ALL_VALUES: &[(&str, &str)] = &[
        ("id", "deep-modules"),
        ("created", "2026-07-27"),
        ("title", "Deep Modules"),
        ("content", ""),
    ];

    #[test]
    fn fill_substitutes_every_placeholder_including_repeats() {
        let template =
            "#meta(id: \"{{id}}\", created: \"{{created}}\")\n= {{id}}\n";
        assert_eq!(
            fill(template, ALL_VALUES).expect("fill"),
            "#meta(id: \"deep-modules\", created: \"2026-07-27\")\n= deep-modules\n"
        );
    }

    #[test]
    fn a_template_without_placeholders_passes_through() {
        assert_eq!(fill("= Plain\n", ALL_VALUES).expect("fill"), "= Plain\n");
    }

    #[test]
    fn an_unknown_placeholder_is_an_error() {
        let result = fill("= {{titel}}\n", ALL_VALUES);
        assert!(
            matches!(result, Err(TemplateError::UnknownPlaceholder(name)) if name == "titel")
        );
    }

    #[test]
    fn an_unterminated_placeholder_is_plain_text() {
        // the ADR's known ceiling: no closing delimiter, nothing to recognize
        assert_eq!(
            fill("= {{titel\n", ALL_VALUES).expect("fill"),
            "= {{titel\n"
        );
    }

    #[test]
    fn stray_braces_stay_text_around_a_real_placeholder() {
        assert_eq!(
            fill("{{ {{id}} }}", ALL_VALUES).expect("fill"),
            "{{ deep-modules }}"
        );
    }

    #[test]
    fn a_substituted_value_is_never_rescanned() {
        // a note legitimately titled "{{weird}}" must survive substitution
        let values = [
            ("id", "x"),
            ("created", "c"),
            ("title", "{{weird}}"),
            ("content", ""),
        ];
        assert_eq!(fill("= {{title}}", &values).expect("fill"), "= {{weird}}");
    }

    #[test]
    fn kebab_ids_lowercase_and_hyphenate() {
        assert_eq!(
            kebab_id("Zettelkasten: An Overview"),
            "zettelkasten-an-overview"
        );
    }

    #[test]
    fn kebab_ids_keep_accented_letters() {
        assert_eq!(kebab_id("L'idée d'été"), "l-idée-d-été");
    }

    #[test]
    fn kebab_ids_leave_time_note_ids_unchanged() {
        // create() relies on this: time notes pass the date as the title
        assert_eq!(kebab_id("2026-07-27"), "2026-07-27");
        assert_eq!(kebab_id("2026-w30"), "2026-w30");
    }

    #[test]
    fn kebab_ids_collapse_and_trim_separators() {
        assert_eq!(kebab_id("  ...spaced --- out!  "), "spaced-out");
        assert_eq!(kebab_id("???"), "");
    }

    fn vault_with(template_name: &str, template: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        for sub in ["templates", "permanent", "time", "capture", "generated"] {
            std::fs::create_dir(dir.path().join(sub))
                .expect("create vault dir");
        }
        std::fs::write(
            dir.path()
                .join("templates")
                .join(format!("{template_name}.typ")),
            template,
        )
        .expect("write template");
        dir
    }

    #[test]
    fn create_writes_the_filled_note_in_its_category_dir() {
        let dir = vault_with("concept", "= {{title}} ({{id}}, {{created}})\n");
        let path = create(
            dir.path(),
            &NoteCategory::Permanent,
            &NoteType::Concept,
            "Deep Modules",
            "2026-07-27",
            "",
        )
        .expect("create");
        assert_eq!(path, dir.path().join("permanent/deep-modules.typ"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read note"),
            "= Deep Modules (deep-modules, 2026-07-27)\n"
        );
    }

    #[test]
    fn the_fixture_time_templates_instantiate_cleanly() {
        // read the real fixture templates, so a placeholder typo in either
        // file fails this build instead of the first "today" in production
        for (name, note_type, id) in [
            ("weekly", NoteType::Weekly, "2026-w31"),
            ("seasonal", NoteType::Seasonal, "2026-autumn"),
        ] {
            let text = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    format!("tests/fixtures/vault/templates/{name}.typ"),
                ),
            )
            .expect("read fixture template");
            let dir = vault_with(name, &text);
            let path = create(
                dir.path(),
                &NoteCategory::Time,
                &note_type,
                id,
                "2026-07-27",
                "",
            )
            .expect("create");
            assert_eq!(path, dir.path().join(format!("time/{id}.typ")));
            let written = std::fs::read_to_string(&path).expect("read note");
            assert!(written.contains(&format!("= {id}")), "{written}");
            assert!(
                !written.contains("{{"),
                "every placeholder filled: {written}"
            );
        }
    }

    #[test]
    fn a_colliding_id_is_refused_and_the_existing_note_untouched() {
        let dir = vault_with("daily", "= {{id}}\n");
        let today = || {
            create(
                dir.path(),
                &NoteCategory::Time,
                &NoteType::Daily,
                "2026-07-27",
                "2026-07-27",
                "",
            )
        };
        let first = today().expect("first create");
        std::fs::write(&first, "= 2026-07-27\nEdited by hand.\n")
            .expect("edit note");
        let result = today();
        assert!(
            matches!(result, Err(TemplateError::AlreadyExists(path)) if path == first)
        );
        assert_eq!(
            std::fs::read_to_string(&first).expect("read note"),
            "= 2026-07-27\nEdited by hand.\n"
        );
    }

    #[test]
    fn a_title_with_no_usable_characters_is_an_error() {
        let dir = vault_with("concept", "");
        let result = create(
            dir.path(),
            &NoteCategory::Permanent,
            &NoteType::Concept,
            "???",
            "2026-07-27",
            "",
        );
        assert!(
            matches!(result, Err(TemplateError::EmptyId(title)) if title == "???")
        );
    }

    #[test]
    fn a_template_typo_prevents_the_file_from_being_written() {
        // the placeholder ADR's core promise: {{titel}} never reaches disk
        let dir = vault_with("concept", "= {{titel}}\n");
        let result = create(
            dir.path(),
            &NoteCategory::Permanent,
            &NoteType::Concept,
            "Note",
            "2026-07-27",
            "",
        );
        assert!(matches!(result, Err(TemplateError::UnknownPlaceholder(_))));
        assert!(!dir.path().join("permanent/note.typ").exists());
    }

    #[test]
    fn a_missing_template_is_unknown_template() {
        let dir = vault_with("concept", "");
        let result = create(
            dir.path(),
            &NoteCategory::Time,
            &NoteType::Daily,
            "2026-07-27",
            "2026-07-27",
            "",
        );
        assert!(
            matches!(result, Err(TemplateError::UnknownTemplate(name)) if name == "daily")
        );
    }

    #[test]
    fn an_unreadable_template_is_io_not_unknown() {
        let dir = vault_with("concept", "");
        // a directory where a file should be: reading fails, but not with NotFound
        std::fs::create_dir(dir.path().join("templates/daily.typ"))
            .expect("create decoy dir");
        let result = create(
            dir.path(),
            &NoteCategory::Time,
            &NoteType::Daily,
            "2026-07-27",
            "2026-07-27",
            "",
        );
        assert!(matches!(result, Err(TemplateError::Io(_, _))));
    }

    #[test]
    fn a_missing_category_dir_is_io() {
        let dir = vault_with("concept", "");
        std::fs::remove_dir(dir.path().join("permanent"))
            .expect("remove category dir");
        let result = create(
            dir.path(),
            &NoteCategory::Permanent,
            &NoteType::Concept,
            "Note",
            "2026-07-27",
            "",
        );
        assert!(
            matches!(result, Err(TemplateError::Io(_, err)) if err.kind() == ErrorKind::NotFound)
        );
    }
}
