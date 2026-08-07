//! The headless capture path: `app --capture` reads a paste on stdin,
//! writes it into the vault and exits, leaving the running app's watcher to
//! notice (`adr/2026-08-capture-headless-second-process.md`). Everything the
//! flag does lives here, so `main` stays three lines of plumbing.

use std::io::Read;
use std::path::PathBuf;

use crate::template;

/// A capture is a paste, not a file transfer. Anything larger is a mistake
/// worth reporting rather than a note worth writing.
const MAX_CAPTURE_BYTES: u64 = 1_048_576;

/// Writes one capture and says where it went. `input` is read to the end —
/// the pipe's whole contents become the note's `== Original` section, the
/// user's own words, untouched.
///
/// Every failure is a message for stderr: this process has no window to
/// show a notice in, and losing a paste silently is the one outcome worth
/// preventing.
pub fn run(
    root: Option<PathBuf>,
    now: &jiff::Zoned,
    input: &mut dyn Read,
) -> Result<PathBuf, String> {
    let root = root.ok_or("no vault: define NOTE_VAULT or HOME")?;
    let content = read_paste(input)?;
    template::create_capture(
        &root,
        &capture_id(now),
        &now.date().to_string(),
        &content,
    )
    .map_err(|err| format!("capture: {err:?}"))
}

/// "capture-2026-08-06-143012" — the clock to the second, which is unique
/// by construction for a hotkey a human presses
/// (`adr/2026-08-capture-timestamp-ids.md`). A same-second collision is an
/// error like any other, per `adr/2026-07-id-collision-is-an-error.md`.
pub fn capture_id(now: &jiff::Zoned) -> String {
    now.strftime("capture-%Y-%m-%d-%H%M%S").to_string()
}

/// The paste, bounded. Reading one byte past the limit is what tells the
/// difference between a paste that just fits and one that was truncated.
fn read_paste(input: &mut dyn Read) -> Result<String, String> {
    let mut content = String::new();
    input
        .take(MAX_CAPTURE_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(|err| format!("capture: unreadable input: {err}"))?;
    if content.len() as u64 > MAX_CAPTURE_BYTES {
        return Err("capture: too large (over 1 MiB)".to_string());
    }
    Ok(content)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::path::Path;

    const NOW: &str = "2026-08-06T14:30:12+02:00[Europe/Paris]";

    #[test]
    fn a_paste_becomes_a_capture_note_carrying_an_open_loop() {
        let vault = vault_with_capture_template();
        let written = run(
            Some(vault.path().to_path_buf()),
            &now(),
            &mut "collé du navigateur".as_bytes(),
        )
        .expect("the capture is written");

        assert_eq!(
            written,
            vault.path().join("capture/capture-2026-08-06-143012.typ")
        );
        let text = std::fs::read_to_string(&written).expect("read it back");
        assert!(
            text.contains(r#"id: "capture-2026-08-06-143012""#),
            "{text}"
        );
        assert!(text.contains(r#"created: "2026-08-06""#), "{text}");
        assert!(text.contains("collé du navigateur"), "{text}");
        assert!(!text.contains("{{"), "every placeholder is filled: {text}");
        // and it arrives as debt: nothing is written under Summary
        assert!(!crate::parse::parse_note(&text).summarized, "{text}");
    }

    #[test]
    fn an_empty_paste_is_still_a_capture() {
        // no required fields means no required content either: the hotkey
        // fires before there is anything to say
        let vault = vault_with_capture_template();
        let written =
            run(Some(vault.path().to_path_buf()), &now(), &mut "".as_bytes())
                .expect("the empty capture is written");
        assert!(written.exists());
    }

    #[test]
    fn a_second_capture_in_the_same_second_is_refused() {
        let vault = vault_with_capture_template();
        let root = Some(vault.path().to_path_buf());
        run(root.clone(), &now(), &mut "premier".as_bytes())
            .expect("the first capture is written");
        let error = run(root, &now(), &mut "second".as_bytes())
            .expect_err("the id is taken");
        assert!(error.contains("AlreadyExists"), "{error}");
    }

    #[test]
    fn without_a_vault_the_paste_is_refused_rather_than_dropped() {
        let error = run(None, &now(), &mut "perdu".as_bytes())
            .expect_err("there is nowhere to write");
        assert!(error.contains("no vault"), "{error}");
    }

    #[test]
    fn a_missing_capture_template_is_reported() {
        let vault = tempfile::tempdir().expect("a temp dir is available");
        std::fs::create_dir_all(vault.path().join("capture"))
            .expect("the category directory is created");
        let error = run(
            Some(vault.path().to_path_buf()),
            &now(),
            &mut "sans modèle".as_bytes(),
        )
        .expect_err("no template to instantiate");
        assert!(error.contains("UnknownTemplate"), "{error}");
    }

    #[test]
    fn input_that_is_not_utf8_is_refused() {
        let vault = vault_with_capture_template();
        let error = run(
            Some(vault.path().to_path_buf()),
            &now(),
            &mut [0xff, 0xfe, 0x00].as_slice(),
        )
        .expect_err("a paste is text");
        assert!(error.contains("unreadable input"), "{error}");
    }

    #[test]
    fn a_paste_larger_than_the_limit_is_refused() {
        let vault = vault_with_capture_template();
        let huge = "x".repeat(MAX_CAPTURE_BYTES as usize + 1);
        let error = run(
            Some(vault.path().to_path_buf()),
            &now(),
            &mut huge.as_bytes(),
        )
        .expect_err("that is a file, not a paste");
        assert!(error.contains("too large"), "{error}");

        // and one byte under it goes through
        let just_fits = "x".repeat(MAX_CAPTURE_BYTES as usize);
        run(
            Some(vault.path().to_path_buf()),
            &now(),
            &mut just_fits.as_bytes(),
        )
        .expect("the limit is inclusive");
    }

    #[test]
    fn the_id_carries_the_clock_down_to_the_second() {
        assert_eq!(capture_id(&now()), "capture-2026-08-06-143012");
    }

    fn now() -> jiff::Zoned {
        NOW.parse().expect("the test clock is a valid timestamp")
    }

    /// A vault with just enough to instantiate a capture: the real fixture
    /// template, so its placeholders and section headings are the ones
    /// shipped.
    fn vault_with_capture_template() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        for sub in ["templates", "capture"] {
            std::fs::create_dir_all(dir.path().join(sub))
                .expect("the directory is created");
        }
        std::fs::copy(
            Path::new("tests/fixtures/vault/templates/capture.typ"),
            dir.path().join("templates/capture.typ"),
        )
        .expect("the fixture capture template is available");
        dir
    }
}
