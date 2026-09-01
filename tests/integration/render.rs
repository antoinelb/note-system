//! `VaultWorld` tests (phase 4): the vault is the typst root, notes reach the
//! shared template through it, and everything outside the vault is refused.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use note_system::render::{
    BodyCache, DEFAULT_SIZE, FragmentCache, RenderError, RenderTheme,
    VaultWorld, render_svg,
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
        RenderTheme::Paper(DEFAULT_SIZE),
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
        VaultWorld::new(
            &vault(),
            outside,
            String::new(),
            RenderTheme::Paper(DEFAULT_SIZE)
        )
        .is_err()
    );
}

#[test]
fn a_dangling_import_surfaces_as_a_compilation_error() {
    let world = VaultWorld::new(
        &vault(),
        &vault().join("permanent/probe.typ"),
        "#import \"/templates/absente.typ\": *\n".to_string(),
        RenderTheme::Paper(DEFAULT_SIZE),
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

    let svg =
        render_svg(&vault(), &note, &text, RenderTheme::Paper(DEFAULT_SIZE))
            .expect("the fixture note renders");
    assert!(svg.starts_with("<svg"), "{}", &svg[..svg.len().min(80)]);
    assert!(svg.ends_with("</svg>"), "{}", &svg[svg.len() - 80..]);
}

#[test]
fn a_note_that_does_not_compile_reports_its_diagnostics() {
    // an unclosed delimiter: guaranteed to fail parsing, not just evaluation
    let note = vault().join("permanent/casse.typ");
    let messages = match render_svg(
        &vault(),
        &note,
        "#let x = (",
        RenderTheme::Paper(DEFAULT_SIZE),
    ) {
        Err(RenderError::Compile(messages)) => messages,
        other => panic!("expected compile diagnostics, got {other:?}"),
    };
    assert!(!messages.is_empty());
    assert!(!messages[0].is_empty(), "a diagnostic carries its message");
}

#[test]
fn a_note_outside_the_vault_is_a_path_error_not_a_compile_error() {
    let error = render_svg(
        &vault(),
        Path::new("/etc/passwd"),
        "",
        RenderTheme::Paper(DEFAULT_SIZE),
    )
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

    let paper =
        render_svg(&vault(), &note, &text, RenderTheme::Paper(DEFAULT_SIZE))
            .expect("the paper column renders");
    assert!(paper.contains("#ffffff"), "paper keeps the white page");
    assert!(paper.contains("#45415a"), "and the light-column ink");

    let dark =
        render_svg(&vault(), &note, &text, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("the dark column renders");
    assert!(!dark.contains("#ffffff"), "no white anywhere (design § 4a)");
    assert!(dark.contains("#c9c4dd"), "the dark-column ink");

    let light =
        render_svg(&vault(), &note, &text, RenderTheme::Light(DEFAULT_SIZE))
            .expect("the light column renders");
    assert!(!light.contains("#ffffff"), "transparent page in-app");
    assert!(light.contains("#45415a"), "the light-column ink");
}

#[test]
fn the_size_input_scales_the_rendered_type() {
    // one signal drives both faces (adr/2026-08-one-font-size-for-source-and-render.md):
    // a bigger size must render taller text, not just change colour inputs.
    // The page width is fixed (14cm in template.typ), so its auto height
    // is what grows with the type.
    let note = vault().join("permanent/zettelkasten.typ");
    let text = "#import \"/templates/template.typ\": *\n\
                #show: note\nbonjour le monde\n";

    let default =
        render_svg(&vault(), &note, text, RenderTheme::Paper(DEFAULT_SIZE))
            .expect("the default size renders");
    let bigger = render_svg(&vault(), &note, text, RenderTheme::Paper(30))
        .expect("a bigger size renders");

    assert_ne!(default, bigger, "a different size must change the output");
    assert!(
        svg_height_pt(&bigger) > svg_height_pt(&default),
        "default {default}\nbigger {bigger}"
    );
}

/// The `height="…pt"` attribute `typst_svg::svg_merged` writes on the root
/// `<svg>` element.
fn svg_height_pt(svg: &str) -> f64 {
    let start = svg
        .find(r#"height=""#)
        .expect("an svg has a height attribute")
        + r#"height=""#.len();
    let rest = &svg[start..];
    let end = rest.find("pt").expect("the height attribute is in points");
    rest[..end]
        .parse()
        .expect("the height attribute is a number")
}

#[test]
fn checklist_items_render_as_task_circles() {
    // `- [ ]` / `- [x]` become the template's task circles
    // (adr/2026-07-checklist-rendering.md); the done colour is the
    // fingerprint of the transformed item
    let note = vault().join("permanent/zettelkasten.typ");
    let text = "#import \"/templates/template.typ\": *\n\
                #show: note\n- [ ] ouvert\n\n- [x] fait\n";

    let dark =
        render_svg(&vault(), &note, text, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("the checklist renders");
    assert!(dark.contains("#6fb08c"), "the dark done circle: {dark}");

    let paper =
        render_svg(&vault(), &note, text, RenderTheme::Paper(DEFAULT_SIZE))
            .expect("the checklist renders on paper too");
    assert!(paper.contains("#4a8a6a"), "the paper done circle");
}

#[test]
fn block_quotes_render_with_the_themes_muted_vertical_rule() {
    let note = vault().join("permanent/zettelkasten.typ");
    let text = "#import \"/templates/template.typ\": *\n\
                #show: note\n\
                #quote(block: true, attribution: [Simone Weil])\
                [Une idée importante.]\n";

    let dark =
        render_svg(&vault(), &note, text, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("the dark quote renders");
    assert!(dark.contains("#6f6a8c"), "the dark muted rule: {dark}");

    let paper =
        render_svg(&vault(), &note, text, RenderTheme::Paper(DEFAULT_SIZE))
            .expect("the paper quote renders");
    assert!(paper.contains("#8b87a0"), "the paper muted rule: {paper}");
}

#[test]
fn a_greater_than_line_renders_the_same_muted_rule_as_an_explicit_quote() {
    // `> ` is now the stored quote syntax, taught to vanilla Typst by a
    // `show par:` rule in the template rather than expanded at Enter-time
    // in the editor (adr/2026-08-greater-than-expands-to-quote.md
    // superseded).
    let note = vault().join("permanent/zettelkasten.typ");
    let text = "#import \"/templates/template.typ\": *\n\
                #show: note\n\
                > Une idée.\n";

    let dark =
        render_svg(&vault(), &note, text, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("the > quote renders");
    assert!(dark.contains("#6f6a8c"), "the dark muted rule: {dark}");

    let paper =
        render_svg(&vault(), &note, text, RenderTheme::Paper(DEFAULT_SIZE))
            .expect("the > quote renders");
    assert!(paper.contains("#8b87a0"), "the paper muted rule: {paper}");
}

#[test]
fn a_greater_than_line_with_a_trailing_attribution_renders_it() {
    let note = vault().join("permanent/zettelkasten.typ");
    let bare = "#import \"/templates/template.typ\": *\n\
                #show: note\n\
                > Une idée simple.\n";
    let attributed = "#import \"/templates/template.typ\": *\n\
                       #show: note\n\
                       > Une idée simple. _Simone Weil_\n";

    let without =
        render_svg(&vault(), &note, bare, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("the bare > quote renders");
    assert!(without.contains("#6f6a8c"), "the muted rule: {without}");

    let with = render_svg(
        &vault(),
        &note,
        attributed,
        RenderTheme::Dark(DEFAULT_SIZE),
    )
    .expect("the attributed > quote renders");
    assert!(with.contains("#6f6a8c"), "the muted rule: {with}");
    // the attribution is emphasised text after the body: its glyphs are
    // extra `<use>` references the bare quote never draws, the only
    // signal available since typst's SVG glyphs are opaque path/`<use>`
    // refs rather than searchable text
    assert!(
        with.matches("<use").count() > without.matches("<use").count(),
        "the attribution's glyphs should render on top of the body"
    );
}

#[test]
fn inline_quotes_are_promoted_to_full_width_block_quotes() {
    let note = vault().join("permanent/zettelkasten.typ");
    let text = "#import \"/templates/template.typ\": *\n\
                #show: note\n\
                #quote(attribution: [Simone Weil])[Une idée importante.]\n";

    let dark =
        render_svg(&vault(), &note, text, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("the dark quote renders");
    assert!(dark.contains("#6f6a8c"), "the dark muted rule: {dark}");

    let paper =
        render_svg(&vault(), &note, text, RenderTheme::Paper(DEFAULT_SIZE))
            .expect("the paper quote renders");
    assert!(paper.contains("#8b87a0"), "the paper muted rule: {paper}");
}

// The cache tests instrument through the filesystem: deleting the template
// makes recompilation impossible, so a successful render can only be a hit.

#[test]
fn a_fragment_hit_serves_the_svg_without_recompiling() {
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    let first = cache
        .render(
            vault.path(),
            &note,
            NOTE_A,
            RenderTheme::Paper(DEFAULT_SIZE),
        )
        .expect("the first render compiles");
    assert!(
        first.starts_with("<svg"),
        "{}",
        &first[..first.len().min(80)]
    );
    remove_template(&vault);
    let second = cache
        .render(
            vault.path(),
            &note,
            NOTE_A,
            RenderTheme::Paper(DEFAULT_SIZE),
        )
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
        .render(
            vault.path(),
            &note,
            NOTE_A,
            RenderTheme::Paper(DEFAULT_SIZE),
        )
        .expect_err("the template is gone");
    assert!(error.contains("file not found"), "{error}");

    // the error entry is served without recompiling: repairing the vault
    // changes nothing while the entry keeps being rendered each generation
    restore_template(&vault);
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_A,
                RenderTheme::Paper(DEFAULT_SIZE)
            )
            .is_err()
    );
    cache.sweep();
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_A,
                RenderTheme::Paper(DEFAULT_SIZE)
            )
            .is_err()
    );

    // two sweeps with no render between evict it, and the repaired vault
    // finally recompiles
    cache.sweep();
    cache.sweep();
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_A,
                RenderTheme::Paper(DEFAULT_SIZE)
            )
            .is_ok()
    );
}

#[test]
fn sweep_drops_what_the_last_generation_never_rendered() {
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    cache
        .render(
            vault.path(),
            &note,
            NOTE_A,
            RenderTheme::Paper(DEFAULT_SIZE),
        )
        .expect("A compiles");
    cache.sweep();
    cache
        .render(
            vault.path(),
            &note,
            NOTE_B,
            RenderTheme::Paper(DEFAULT_SIZE),
        )
        .expect("B compiles");
    cache.sweep();
    remove_template(&vault);

    // B survived its generation's sweep; A was evicted by B's sweep
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_B,
                RenderTheme::Paper(DEFAULT_SIZE)
            )
            .is_ok()
    );
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_A,
                RenderTheme::Paper(DEFAULT_SIZE)
            )
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
        .render(vault.path(), &a, NOTE_A, RenderTheme::Paper(DEFAULT_SIZE))
        .expect("a compiles");
    remove_template(&vault);

    // same source under another path is a distinct fragment, not a hit
    assert!(
        cache
            .render(vault.path(), &a, NOTE_A, RenderTheme::Paper(DEFAULT_SIZE))
            .is_ok()
    );
    assert!(
        cache
            .render(vault.path(), &b, NOTE_A, RenderTheme::Paper(DEFAULT_SIZE))
            .is_err()
    );
}

#[test]
fn fragments_are_keyed_by_theme_too() {
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    cache
        .render(vault.path(), &note, NOTE_A, RenderTheme::Dark(DEFAULT_SIZE))
        .expect("the dark render compiles");
    remove_template(&vault);

    // the same source in another theme is a distinct entry, not a hit
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_A,
                RenderTheme::Dark(DEFAULT_SIZE)
            )
            .is_ok()
    );
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_A,
                RenderTheme::Light(DEFAULT_SIZE)
            )
            .is_err()
    );
}

#[test]
fn fragments_are_keyed_by_size_too() {
    // the size travels inside `RenderTheme`, so it rides the same `Hash`
    // the cache keys on — a size change must miss exactly like a theme
    // change does (adr/2026-08-one-font-size-for-source-and-render.md)
    let vault = temp_vault();
    let note = vault.path().join("permanent/a.typ");
    let mut cache = FragmentCache::default();

    cache
        .render(vault.path(), &note, NOTE_A, RenderTheme::Dark(DEFAULT_SIZE))
        .expect("the default-size render compiles");
    remove_template(&vault);

    // the same source and theme at another size is a distinct entry
    assert!(
        cache
            .render(
                vault.path(),
                &note,
                NOTE_A,
                RenderTheme::Dark(DEFAULT_SIZE)
            )
            .is_ok()
    );
    assert!(
        cache
            .render(vault.path(), &note, NOTE_A, RenderTheme::Dark(30))
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
            RenderTheme::Paper(DEFAULT_SIZE),
        )
        .expect_err("a note outside the vault cannot virtualize");
    assert!(!error.is_empty());
}

// -- the body cache: whole notes for the table's body zoom -------------------
// (adr/2026-08-body-cache-per-note-svg.md) — instrumented the same way:
// with the template gone, a successful render can only be a hit.

#[test]
fn a_body_hit_serves_the_svg_without_recompiling_or_rereading() {
    let vault = temp_vault();
    write_note(&vault, "a", NOTE_A);
    let mut cache = BodyCache::default();
    let note = Path::new("permanent/a.typ");

    let first = cache
        .render(vault.path(), note, RenderTheme::Paper(DEFAULT_SIZE))
        .expect("the first render reads and compiles");
    assert!(
        first.starts_with("<svg"),
        "{}",
        &first[..first.len().min(80)]
    );
    // file and template both vanish: only a cache hit can still answer
    remove_template(&vault);
    std::fs::remove_file(vault.path().join("permanent/a.typ"))
        .expect("the note is removed");
    let second = cache
        .render(vault.path(), note, RenderTheme::Paper(DEFAULT_SIZE))
        .expect("a hit must not reread or recompile");
    assert_eq!(first, second);
}

#[test]
fn invalidate_drops_both_theme_columns_of_the_one_note() {
    let vault = temp_vault();
    write_note(&vault, "a", NOTE_A);
    write_note(&vault, "b", NOTE_B);
    let mut cache = BodyCache::default();
    let a = Path::new("permanent/a.typ");
    let b = Path::new("permanent/b.typ");
    for theme in [
        RenderTheme::Dark(DEFAULT_SIZE),
        RenderTheme::Light(DEFAULT_SIZE),
    ] {
        cache
            .render(vault.path(), a, theme)
            .expect("a compiles in both themes");
    }
    cache
        .render(vault.path(), b, RenderTheme::Dark(DEFAULT_SIZE))
        .expect("b compiles");

    remove_template(&vault);
    cache.invalidate(a);
    // both of a's columns recompile — and fail, the template being gone —
    // while b's untouched entry still answers
    assert!(
        cache
            .render(vault.path(), a, RenderTheme::Dark(DEFAULT_SIZE))
            .is_err()
    );
    assert!(
        cache
            .render(vault.path(), a, RenderTheme::Light(DEFAULT_SIZE))
            .is_err()
    );
    assert!(
        cache
            .render(vault.path(), b, RenderTheme::Dark(DEFAULT_SIZE))
            .is_ok()
    );
}

#[test]
fn clear_empties_the_whole_cache() {
    let vault = temp_vault();
    write_note(&vault, "a", NOTE_A);
    let mut cache = BodyCache::default();
    let a = Path::new("permanent/a.typ");
    cache
        .render(vault.path(), a, RenderTheme::Paper(DEFAULT_SIZE))
        .expect("a compiles");

    remove_template(&vault);
    cache.clear();
    assert!(
        cache
            .render(vault.path(), a, RenderTheme::Paper(DEFAULT_SIZE))
            .is_err(),
        "a rescan's clear forgets every entry"
    );
}

#[test]
fn an_unreadable_note_caches_its_error_until_invalidated() {
    let vault = temp_vault();
    let mut cache = BodyCache::default();
    let note = Path::new("permanent/absent.typ");

    let error = cache
        .render(vault.path(), note, RenderTheme::Paper(DEFAULT_SIZE))
        .expect_err("nothing to read");
    assert!(error.starts_with("body:"), "{error}");

    // the error entry is served without rereading: the note appearing on
    // disk changes nothing until the watcher invalidates it
    write_note(&vault, "absent", NOTE_A);
    assert!(
        cache
            .render(vault.path(), note, RenderTheme::Paper(DEFAULT_SIZE))
            .is_err()
    );
    cache.invalidate(note);
    assert!(
        cache
            .render(vault.path(), note, RenderTheme::Paper(DEFAULT_SIZE))
            .is_ok()
    );
}

fn write_note(vault: &tempfile::TempDir, id: &str, text: &str) {
    let dir = vault.path().join("permanent");
    std::fs::create_dir_all(&dir).expect("the category dir is creatable");
    std::fs::write(dir.join(format!("{id}.typ")), text)
        .expect("the note is writable");
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
    VaultWorld::new(&vault(), &note, text, RenderTheme::Paper(DEFAULT_SIZE))
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

#[test]
fn nested_checklist_items_render_indented() {
    // a nested `- [ ]` is not wrapped in a child list by typst — it lands
    // as a further `list.item` sibling trailing the label in the same
    // body sequence, so template.typ splits it off and indents it by 1em
    // (adr/2026-08-nested-checklist-indentation.md). The offset is the
    // point, so this walks the SVG's own transform stack instead of
    // asserting on colour.
    let note = vault().join("permanent/zettelkasten.typ");
    // one Rust string segment for the whole checklist: a `\` line
    // continuation strips all of the next physical line's leading
    // whitespace, which would destroy the markdown nesting indent below
    let text = "#import \"/templates/template.typ\": *\n\
                #show: note\n- [ ] top\n  - [ ] nested one\n  - [ ] nested two\n- [x] top two\n";
    let svg =
        render_svg(&vault(), &note, text, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("the nested checklist renders");

    let offsets = checkbox_circle_x_offsets(&svg);
    assert_eq!(
        offsets.len(),
        4,
        "two top-level circles and two nested ones: {offsets:?}"
    );
    let (top1, nested1, nested2, top2) =
        (offsets[0], offsets[1], offsets[2], offsets[3]);

    assert!(
        (top1 - top2).abs() < 0.5,
        "the two top-level circles share one indent: {offsets:?}"
    );
    assert!(
        (nested1 - nested2).abs() < 0.5,
        "the two nested circles share one indent: {offsets:?}"
    );
    assert!(
        nested1 > top1 + 1.0,
        "nested circles should sit strictly right of the top-level ones: {offsets:?}"
    );
}

#[test]
fn a_done_parent_does_not_strike_an_open_nested_child() {
    // strike() runs on the parent's own label only; the trailing
    // list.item children are spliced back in afterwards, so a done
    // parent must not mute or strike an open nested child
    // (adr/2026-08-nested-checklist-indentation.md)
    let note = vault().join("permanent/zettelkasten.typ");
    let text = "#import \"/templates/template.typ\": *\n\
                #show: note\n- [x] top\n  - [ ] nested\n";
    let svg =
        render_svg(&vault(), &note, text, RenderTheme::Dark(DEFAULT_SIZE))
            .expect("renders");

    let offsets = checkbox_circle_x_offsets(&svg);
    assert_eq!(
        offsets.len(),
        2,
        "one parent circle, one nested one: {offsets:?}"
    );
    assert!(
        offsets[1] > offsets[0] + 1.0,
        "the open child still indents under the done parent: {offsets:?}"
    );

    let body = svg.split("<defs").next().unwrap_or(&svg);
    let strike_lines = body.matches("d=\"M 0 0h").count();
    assert_eq!(
        strike_lines, 1,
        "only the done parent's label is struck, not the open child: {body}"
    );
}

/// Walks the merged SVG's `<g transform="translate(...)">` stack — the
/// content before `<defs>`, where glyph outlines live and would otherwise
/// be mistaken for circles — and returns the cumulative x offset of every
/// checkbox circle path, in document order. `check()` in template.typ
/// always emits a `d` starting `M 0 0m` for both the open and done
/// variants, while a strike-through rule starts `M 0 0h`, so the two are
/// never conflated.
fn checkbox_circle_x_offsets(svg: &str) -> Vec<f64> {
    let body = svg.split("<defs").next().unwrap_or(svg);
    let mut stack = vec![(0.0_f64, 0.0_f64)];
    let mut offsets = Vec::new();
    for piece in body.split('<').skip(1) {
        let Some(end) = piece.find('>') else {
            continue;
        };
        let tag = &piece[..end];
        if tag.starts_with('/') {
            if stack.len() > 1 {
                stack.pop();
            }
            continue;
        }
        let self_closing = tag.trim_end().ends_with('/');
        let tag_body = tag.trim_end().trim_end_matches('/');
        let mut parts = tag_body.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let attrs = parts.next().unwrap_or("");
        let (parent_x, parent_y) = *stack.last().unwrap_or(&(0.0, 0.0));
        let (dx, dy) = translate_offset(attrs);
        let (x, y) = (parent_x + dx, parent_y + dy);
        if name == "path" && attrs.contains("d=\"M 0 0m") {
            offsets.push(x);
        }
        if !self_closing {
            stack.push((x, y));
        }
    }
    offsets
}

/// The `(x, y)` translation out of a `transform="translate(x [y])"`
/// attribute, or `(0.0, 0.0)` for any other transform (a text run's
/// `matrix(..)`) or none at all — this test only follows indentation
/// carried by `translate`, which is all `block(inset: ..)` ever emits.
fn translate_offset(attrs: &str) -> (f64, f64) {
    let Some(start) = attrs
        .find("transform=\"translate(")
        .map(|i| i + "transform=\"translate(".len())
    else {
        return (0.0, 0.0);
    };
    let Some(rel_end) = attrs[start..].find(')') else {
        return (0.0, 0.0);
    };
    let nums: Vec<f64> = attrs[start..start + rel_end]
        .split_whitespace()
        .filter_map(|n| n.parse().ok())
        .collect();
    (
        nums.first().copied().unwrap_or(0.0),
        nums.get(1).copied().unwrap_or(0.0),
    )
}
