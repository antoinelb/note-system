use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::layout::Abs;
use typst::syntax::{
    FileId, RootedPath, Source, VirtualPath, VirtualRoot, VirtualizeError,
};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;
use typst_svg::SvgOptions;

static FONTS: LazyLock<FontStore> = LazyLock::new(|| {
    let mut font_store = FontStore::new();
    font_store.extend(typst_kit::fonts::embedded());
    // system fonts too, so templates can use the same families the vanilla
    // typst CLI would find (it scans the system by default)
    font_store.extend(typst_kit::fonts::system());
    font_store
});
static LIBRARY: LazyLock<LazyHash<Library>> =
    LazyLock::new(|| LazyHash::new(Library::default()));

pub struct VaultWorld {
    root: PathBuf,
    main: FileId,
    source: Source,
}

impl VaultWorld {
    pub fn new(
        root: &Path,
        note: &Path,
        text: String,
    ) -> Result<VaultWorld, VirtualizeError> {
        let vpath = VirtualPath::virtualize(root, note)?;
        let main = RootedPath::new(VirtualRoot::Project, vpath).intern();
        let source = Source::new(main, text);
        Ok(VaultWorld {
            root: root.to_path_buf(),
            main,
            source,
        })
    }

    fn read(&self, id: FileId) -> FileResult<Vec<u8>> {
        match id.root() {
            VirtualRoot::Project => {
                let path = faults::realize(id.vpath(), &self.root)?;
                std::fs::read(&path)
                    .map_err(|err| FileError::from_io(err, &path))
            }
            VirtualRoot::Package(spec) => {
                Err(FileError::Package(PackageError::Other(Some(
                    format!("{spec} - the vault doesn't use packages").into(),
                ))))
            }
        }
    }
}

impl World for VaultWorld {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }

    fn book(&self) -> &LazyHash<FontBook> {
        FONTS.book()
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            Ok(self.source.clone())
        } else {
            Ok(Source::new(id, String::from_utf8(self.read(id)?)?))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.read(id).map(Bytes::new)
    }

    fn font(&self, index: usize) -> Option<Font> {
        FONTS.font(index)
    }

    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

#[derive(Debug)]
pub enum RenderError {
    Path(VirtualizeError),
    Compile(Vec<String>),
}

impl From<VirtualizeError> for RenderError {
    fn from(error: VirtualizeError) -> RenderError {
        RenderError::Path(error)
    }
}

/// Per-block SVG fragments for the hybrid editor — the successor
/// adr/2026-07-svg-cache-per-path.md predicted, decided in
/// adr/2026-07-block-segmentation-parbreak-tiling.md. Keyed by note path +
/// fragment source, so an unchanged block never recompiles; errors are
/// cached too, because a failing block would otherwise recompile on every
/// re-render. `sweep`, called at every resegmentation, drops what the
/// current generation never rendered, which bounds the map at the open
/// note's block count.
#[derive(Debug, Default)]
pub struct FragmentCache {
    entries: HashMap<u64, Result<String, String>>,
    touched: HashSet<u64>,
}

impl FragmentCache {
    pub fn render(
        &mut self,
        root: &Path,
        note: &Path,
        source: &str,
    ) -> Result<String, String> {
        let key = hash_fragment(note, source);
        self.touched.insert(key);
        self.entries
            .entry(key)
            .or_insert_with(|| {
                render_svg(root, note, source).map_err(describe)
            })
            .clone()
    }

    pub fn sweep(&mut self) {
        self.entries.retain(|key, _| self.touched.contains(key));
        self.touched.clear();
    }
}

/// One line per diagnostic: the fragment's error shows inline in its block
/// slot, where a `Debug` dump of the enum would be noise.
fn describe(error: RenderError) -> String {
    match error {
        RenderError::Path(error) => format!("{error:?}"),
        RenderError::Compile(messages) => messages.join("\n"),
    }
}

pub fn render_svg(
    root: &Path,
    note: &Path,
    text: &str,
) -> Result<String, RenderError> {
    let world = VaultWorld::new(root, note, text.to_string())?;

    match typst::compile::<PagedDocument>(&world).output {
        Ok(doc) => Ok(typst_svg::svg_merged(
            &doc,
            &SvgOptions::default(),
            Abs::pt(0.0),
        )),
        Err(errors) => Err(RenderError::Compile(
            errors.into_iter().map(|e| e.message.to_string()).collect(),
        )),
    }
}

/// The path joins the hash because fragments compile under their note's
/// path: two notes could hold byte-identical blocks whose relative
/// resolution differs. Stable only within this process: `DefaultHasher` may
/// change across Rust releases, so these hashes must never be persisted to
/// `.index/`.
fn hash_fragment(note: &Path, source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    note.hash(&mut hasher);
    source.hash(&mut hasher);
    hasher.finish()
}

/// Fault injection for the one error path no real Linux input reaches.
///
/// `VirtualPath::realize` fails only when a segment maps to something other
/// than exactly one normal path component — Windows drive letters and
/// reserved names. Normalized segments cannot contain `/`, so on Linux the
/// error branch of the `?` in `read` is unreachable through any real path.
/// Outside `cfg(test)` the function here is the identity, so the shipped
/// call is the one the tests exercise. Excluded from coverage for the same
/// reason as `index::faults`: it is scaffolding, and measuring it would only
/// measure whichever arm this build compiled.
#[cfg_attr(coverage_nightly, coverage(off))]
mod faults {
    use std::path::{Path, PathBuf};

    use typst::syntax::{RealizeError, VirtualPath};

    #[cfg(not(test))]
    pub(super) fn realize(
        vpath: &VirtualPath,
        root: &Path,
    ) -> Result<PathBuf, RealizeError> {
        vpath.realize(root)
    }

    #[cfg(test)]
    pub(super) use armed::*;

    #[cfg(test)]
    mod armed {
        use std::cell::Cell;

        use super::*;

        // a single fault site, so a flag rather than index::faults' enum
        thread_local! {
            static ARMED: Cell<bool> = const { Cell::new(false) };
        }

        /// Arms the fault until the returned guard drops, so a panicking
        /// test cannot leak it into the next test on the same thread.
        pub(in crate::render) fn arm() -> Guard {
            ARMED.with(|armed| armed.set(true));
            Guard
        }

        pub(in crate::render) struct Guard;

        impl Drop for Guard {
            fn drop(&mut self) {
                ARMED.with(|armed| armed.set(false));
            }
        }

        pub(in crate::render) fn realize(
            vpath: &VirtualPath,
            root: &Path,
        ) -> Result<PathBuf, RealizeError> {
            if ARMED.with(|armed| armed.get()) {
                Err(RealizeError::Invalid("armed test fault".into()))
            } else {
                vpath.realize(root)
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::str::FromStr;

    use typst::syntax::package::PackageSpec;

    use super::*;

    // These three tests exist so one compilation copy of `read` covers it
    // entirely: llvm-cov folds the unit and integration copies by keeping
    // the best single copy, and only this copy can reach the armed branch.

    #[test]
    fn a_project_file_is_read_through_the_sandbox() {
        let bytes = fixture_world()
            .read(file_id("/templates/template.typ"))
            .expect("the template exists in the fixture vault");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn a_package_file_is_refused_without_a_filesystem_read() {
        let spec = PackageSpec::from_str("@preview/example:0.1.0")
            .expect("a well-formed package spec");
        let id = RootedPath::new(
            VirtualRoot::Package(spec),
            VirtualPath::new("/lib.typ").expect("a valid virtual path"),
        )
        .intern();

        let error = fixture_world().read(id).unwrap_err();
        assert!(matches!(error, FileError::Package(_)), "{error:?}");
    }

    #[test]
    fn an_unrealizable_path_is_an_error_not_a_panic() {
        // realize cannot fail on Linux — a normalized segment is always
        // exactly one normal component — so the branch is reached by arming
        // the fault
        let world = fixture_world();
        let _guard = faults::arm();
        let error =
            world.read(file_id("/templates/template.typ")).unwrap_err();
        assert!(matches!(error, FileError::Realize(_)), "{error:?}");
    }

    fn fixture_world() -> VaultWorld {
        let vault =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
        let note = vault.join("permanent/zettelkasten.typ");
        // the text is irrelevant to `read`; an empty buffer keeps the
        // fixture free of a disk read
        VaultWorld::new(&vault, &note, String::new())
            .expect("a fixture path inside the vault virtualizes")
    }

    fn file_id(virtual_path: &str) -> FileId {
        RootedPath::new(
            VirtualRoot::Project,
            VirtualPath::new(virtual_path).expect("a valid virtual path"),
        )
        .intern()
    }
}
