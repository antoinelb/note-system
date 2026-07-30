//! `VaultWorld` tests (phase 4): the vault is the typst root, notes reach the
//! shared template through it, and everything outside the vault is refused.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use note_system::render::{
    FragmentCache, RenderError, RenderTheme, VaultWorld, render_svg,
};
use typst::World;
use typst::diag::FileError;
use typst::syntax::package::PackageSpec;
use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
use typst_layout::PagedDocument;

#[test]
fn a_note_compiles_through_its_root_absolute_template_import() {
    // the phase-1 contract: the vault is the root, so `#import
    // "/templates/template.typ"` resolves without the note knowing its depth
    let world = world_for("permanent/zettelkasten.typ");
    let document = compile(&world).expect("fixture note should compile");
    assert_eq!(document.pages().len(), 1);
}

#[test]
fn the_note_text_comes_from_memory_not_from_disk() {
    // what lets phase 5 render an unsaved buffer: the text handed to `new` wins
    // over the bytes on disk for the main file
    let world = VaultWorld::new(
        &vault(),
        &vault().join("permanent/zettelkasten.typ"),
        "#import \"/templates/template.typ\": *\n= Unsaved\n".to_string(),
        RenderTheme::Paper,
    )
    .expect("a path inside the vault virtualizes");

    let source = world.source(world.main()).expect("main is always readable");
    assert!(source.text().contains("Unsaved"));
    assert!(!source.text().contains("Luhmann"));
}

#[test]
fn a_missing_file_is_reported_with_the_path_that_was_searched() {
    let world = world_for("permanent/zettelkasten.typ");
    let error = world
        .file(file_id("/permanent/pas-de-note.typ"))
        .unwrap_err();
    match error {
        FileError::NotFound(path) => {
            assert!(path.ends_with("permanent/pas-de-note.typ"), "{path:?}")
        }
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn a_file_that_is_not_utf8_is_refused_as_a_source() {
    // `file` hands back the raw bytes; only `source` has to decode them
    let world = world_for("permanent/zettelkasten.typ");
    let id = file_id("/permanent/non-utf8-binary.bin");

    assert!(world.file(id).is_ok(), "raw bytes are always readable");
    assert!(matches!(world.source(id), Err(FileError::InvalidUtf8)));
}

#[test]
fn a_package_root_is_refused_without_touching_the_filesystem() {
    // the vault is self-contained; a package id must not fall through to a read
    let world = world_for("permanent/zettelkasten.typ");
    let spec = PackageSpec::from_str("@preview/example:0.1.0")
        .expect("a well-formed package spec");
    let id = RootedPath::new(
        VirtualRoot::Package(spec),
        VirtualPath::new("/lib.typ").expect("a valid virtual path"),
    )
    .intern();

    let message = match world.file(id).unwrap_err() {
        FileError::Package(error) => error.to_string(),
        other => panic!("expected a package error, got {other:?}"),
    };
    assert!(message.contains("@preview/example:0.1.0"), "{message}");
    assert!(message.contains("packages"), "{message}");
}

#[test]
fn a_note_outside_the_vault_is_refused_at_construction() {
    let outside = Path::new("/etc/passwd");
    assert!(
        VaultWorld::new(&vault(), outside, String::new(), RenderTheme::Paper)
            .is_err()
    );
}

#[test]
fn a_dangling_import_surfaces_as_a_compilation_error() {
    let world = VaultWorld::new(
        &vault(),
        &vault().join("permanent/probe.typ"),
        "#import \"/templates/absente.typ\": *\n".to_string(),
        RenderTheme::Paper,
    )
    .expect("a path inside the vault virtualizes");

    let errors = compile(&world).expect_err("the import cannot resolve");
    assert!(
        errors.iter().any(|e| e.contains("file not found")),
        "{errors:?}"
    );
}

#[test]
fn the_world_has_no_clock() {
    // deliberate, not a stub: rendering must stay a pure function of the
    // vault's bytes or the content-hashed SVG cache goes silently stale
    // (adr/2026-07-embedded-typst-world.md)
    let world = world_for("permanent/zettelkasten.typ");
    assert!(world.today(None).is_none());
}

#[test]
fn fonts_are_available_so_text_can_be_laid_out() {
    let world = world_for("permanent/zettelkasten.typ");
    assert!(world.book().families().next().is_some());
    assert!(world.font(0).is_some());
}

#[test]
fn a_valid_note_renders_to_svg_markup() {
    let note = vault().join("permanent/zettelkasten.typ");
    let text =
        std::fs::read_to_string(&note).expect("the fixture note is readable");

    let svg = render_svg(&vault(), &note, &text, RenderTheme::Paper)
        .expect("the fixture note renders");
    assert!(svg.starts_with("<svg"), "{}", &svg[..svg.len().min(80)]);
    assert!(svg.ends_with("</svg>"), "{}", &svg[svg.len() - 80..]);
}

#[test]
fn a_note_that_does_not_compile_reports_its_diagnostics() {
    // an unclosed delimiter: guaranteed to fail parsing, not just evaluation
    let note = vault().join("permanent/casse.typ");
    let messages =
        match render_svg(&vault(), &note, "#let x = (", RenderTheme::Paper) {
            Err(RenderError::Compile(messages)) => messages,
            other => panic!("expected compile diagnostics, got {other:?}"),
        };
    assert!(!messages.is_empty());
    assert!(!messages[0].is_empty(), "a diagnostic carries its message");
}

#[test]
fn a_note_outside_the_vault_is_a_path_error_not_a_compile_error() {
    let error =
        render_svg(&vault(), Path::new("/etc/passwd"), "", RenderTheme::Paper)
            .unwrap_err();
    assert!(matches!(error, RenderError::Path(_)), "{error:?}");
}

#[test]
fn the_theme_input_picks_the_templates_palette_column() {
    // the values under assertion are the template's own palette columns
    // (tests/fixtures/vault/templates/template.typ)
    let note = vault().join("permanent/zettelkasten.typ");
    let text =
        std::fs::read_to_string(&note).expect("the fixture note is readable");

    let paper = render_svg(&vault(), &note, &text, RenderTheme::Paper)
        .expect("the paper column renders");
    assert!(paper.contains("#ffffff"), "paper keeps the white page");
    assert!(paper.contains("#45415a"), "and the light-column ink");

    let dark = render_svg(&vault(), &note, &text, RenderTheme::Dark)
        .expect("the dark column renders");
    assert!(!dark.contains("#ffffff"), "no white anywhere (design § 4a)");
    assert!(dark.contains("#c9c4dd"), "the dark-column ink");

    let light = render_svg(&vault(), &note, &text, RenderTheme::Light)
        .expect("the light column renders");
    assert!(!light.contains("#ffffff"), "transparent page in-app");
    assert!(light.contains("#45415a"), "the light-column ink");
}

// The cache tests instrument through the filesystem: deleting the template
// makes recompilation impossible, so a successful render can only be a hit.

#[test]
fn a_fragment_hit_serves_the_svg_without_recompiling() {
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    let first = cache
        .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
        .expect("the first render compiles");
    assert!(
        first.starts_with("<svg"),
        "{}",
        &first[..first.len().min(80)]
    );
    remove_template(&vault);
    let second = cache
        .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
        .expect("a hit must not recompile");
    assert_eq!(first, second);
}

#[test]
fn a_fragment_error_is_cached_until_swept() {
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    remove_template(&vault);
    let error = cache
        .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
        .expect_err("the template is gone");
    assert!(error.contains("file not found"), "{error}");

    // the error entry is served without recompiling: repairing the vault
    // changes nothing while the entry keeps being rendered each generation
    restore_template(&vault);
    assert!(
        cache
            .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
            .is_err()
    );
    cache.sweep();
    assert!(
        cache
            .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
            .is_err()
    );

    // two sweeps with no render between evict it, and the repaired vault
    // finally recompiles
    cache.sweep();
    cache.sweep();
    assert!(
        cache
            .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
            .is_ok()
    );
}

#[test]
fn sweep_drops_what_the_last_generation_never_rendered() {
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    cache
        .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
        .expect("A compiles");
    cache.sweep();
    cache
        .render(vault.path(), &note, NOTE_B, RenderTheme::Paper)
        .expect("B compiles");
    cache.sweep();
    remove_template(&vault);

    // B survived its generation's sweep; A was evicted by B's sweep
    assert!(
        cache
            .render(vault.path(), &note, NOTE_B, RenderTheme::Paper)
            .is_ok()
    );
    assert!(
        cache
            .render(vault.path(), &note, NOTE_A, RenderTheme::Paper)
            .is_err()
    );
}

#[test]
fn fragments_are_keyed_by_note_path_as_well_as_source() {
    let vault = temp_vault();
    let a = vault.path().join("permanent/a.typ");
    let b = vault.path().join("permanent/b.typ");
    let mut cache = FragmentCache::default();

    cache
        .render(vault.path(), &a, NOTE_A, RenderTheme::Paper)
        .expect("a compiles");
    remove_template(&vault);

    // same source under another path is a distinct fragment, not a hit
    assert!(
        cache
            .render(vault.path(), &a, NOTE_A, RenderTheme::Paper)
            .is_ok()
    );
    assert!(
        cache
            .render(vault.path(), &b, NOTE_A, RenderTheme::Paper)
            .is_err()
    );
}

#[test]
fn fragments_are_keyed_by_theme_too() {
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    cache
        .render(vault.path(), &note, NOTE_A, RenderTheme::Dark)
        .expect("the dark render compiles");
    remove_template(&vault);

    // the same source in another theme is a distinct entry, not a hit
    assert!(
        cache
            .render(vault.path(), &note, NOTE_A, RenderTheme::Dark)
            .is_ok()
    );
    assert!(
        cache
            .render(vault.path(), &note, NOTE_A, RenderTheme::Light)
            .is_err()
    );
}

#[test]
fn a_fragment_outside_the_vault_reports_the_path_error() {
    let vault = temp_vault();
    let mut cache = FragmentCache::default();

    let error = cache
        .render(
            vault.path(),
            Path::new("/etc/passwd"),
            NOTE_A,
            RenderTheme::Paper,
        )
        .expect_err("a note outside the vault cannot virtualize");
    assert!(!error.is_empty());
}

const NOTE_A: &str = "#import \"/templates/template.typ\": *\n= A\n";
const NOTE_B: &str = "#import \"/templates/template.typ\": *\n= B\n";

/// A vault containing only the shared template: the cache tests provide note
/// text from memory, so no note file has to exist on disk.
fn temp_vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp directory is available");
    std::fs::create_dir(dir.path().join("templates"))
        .expect("the templates directory is creatable");
    restore_template(&dir);
    dir
}

fn remove_template(vault: &tempfile::TempDir) {
    std::fs::remove_file(vault.path().join("templates/template.typ"))
        .expect("the template exists to be removed");
}

fn restore_template(dir: &tempfile::TempDir) {
    std::fs::copy(
        vault().join("templates/template.typ"),
        dir.path().join("templates/template.typ"),
    )
    .expect("the fixture template is copyable");
}

fn world_for(relative: &str) -> VaultWorld {
    let note = vault().join(relative);
    let text = std::fs::read_to_string(&note)
        .unwrap_or_else(|e| panic!("cannot read fixture {note:?}: {e}"));
    VaultWorld::new(&vault(), &note, text, RenderTheme::Paper)
        .expect("a fixture path inside the vault virtualizes")
}

fn compile(world: &VaultWorld) -> Result<PagedDocument, Vec<String>> {
    typst::compile::<PagedDocument>(world)
        .output
        .map_err(|errors| {
            errors.iter().map(|e| e.message.to_string()).collect()
        })
}

fn file_id(virtual_path: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(virtual_path).expect("a valid virtual path"),
    )
    .intern()
}

fn vault() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
}
