use std::{ffi::OsString, path::PathBuf};

pub fn vault_path() -> Option<PathBuf> {
    let note_vault = std::env::var_os("NOTE_VAULT");
    let home = std::env::var_os("HOME");
    resolve_vault_path(note_vault, home)
}

fn resolve_vault_path(
    note_vault: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    match (note_vault, home) {
        (Some(path), _) if !path.is_empty() => Some(PathBuf::from(path)),
        (_, Some(path)) if !path.is_empty() => {
            Some(PathBuf::from(path).join("documents/notes"))
        }
        (_, _) => None,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;

    fn os(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn note_vault_wins_over_the_home_default() {
        assert_eq!(
            resolve_vault_path(os("/srv/notes"), os("/home/antoine")),
            Some(PathBuf::from("/srv/notes"))
        );
    }

    #[test]
    fn without_note_vault_the_default_hangs_off_home() {
        assert_eq!(
            resolve_vault_path(None, os("/home/antoine")),
            Some(PathBuf::from("/home/antoine/documents/notes"))
        );
    }

    #[test]
    fn an_empty_note_vault_means_unset_not_empty_path() {
        // a bare `NOTE_VAULT=` in a shell profile is not a request to open
        // the current directory as a vault
        assert_eq!(
            resolve_vault_path(os(""), os("/home/antoine")),
            Some(PathBuf::from("/home/antoine/documents/notes"))
        );
    }

    #[test]
    fn an_empty_home_never_yields_a_relative_path() {
        // PathBuf::from("").join("documents/notes") is "documents/notes",
        // which would silently open a vault relative to the launch directory
        assert_eq!(resolve_vault_path(None, os("")), None);
        assert_eq!(resolve_vault_path(os(""), os("")), None);
    }

    #[test]
    fn nothing_in_the_environment_is_an_explicit_none() {
        assert_eq!(resolve_vault_path(None, None), None);
    }

    #[test]
    fn a_non_utf8_vault_path_survives_byte_for_byte() {
        // the reason this reads OsString rather than String: a Linux path is
        // bytes, and going through String would drop this one entirely
        let raw = OsString::from_vec(vec![0x2f, 0x76, 0xff, 0x6c]);
        assert_eq!(
            resolve_vault_path(Some(raw.clone()), os("/home/antoine")),
            Some(PathBuf::from(raw))
        );
    }

    #[test]
    fn the_wrapper_reads_note_vault_and_home_in_that_order() {
        // a deliberately weak assertion: `set_var` is unsafe in edition 2024
        // and process-global, so this checks the wiring against the ambient
        // environment and leaves the branches to the tests above
        assert_eq!(
            vault_path(),
            resolve_vault_path(
                std::env::var_os("NOTE_VAULT"),
                std::env::var_os("HOME")
            )
        );
    }
}
