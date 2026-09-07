use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Dict, Duration, IntoValue};
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
/// The prose size the app starts at — 18px, the reading-scale bump's body
/// size (adr/2026-07-reading-scale-bumped.md), expressed once so the UI's
/// default signal and every test fixture read the same number
/// (adr/2026-08-one-font-size-for-source-and-render.md).
pub const DEFAULT_SIZE: u16 = 18;

/// The template reads `sys.inputs.theme` and `sys.inputs.size` and derives
/// its whole palette and type scale from them, so both the app's colour
/// column and its prose size travel as compile inputs — the only channel a
/// template has, since it cannot consume CSS variables
/// (adr/2026-07-note-rendering-theme-input.md,
/// adr/2026-08-one-font-size-for-source-and-render.md). `size` arrives in
/// CSS pixels, the app's one dial, and is converted to points here at the
/// template's own 1px = 0.75pt.
fn themed_library(theme: &str, size: u16) -> LazyHash<Library> {
    let mut inputs = Dict::new();
    inputs.insert("theme".into(), theme.into_value());
    inputs.insert("size".into(), (f64::from(size) * 0.75).into_value());
    LazyHash::new(Library::builder().with_inputs(inputs).build())
}

/// One `Library` per (colour, size) pair the app has asked for.
type LibraryMemo = Mutex<HashMap<(&'static str, u16), Arc<LazyHash<Library>>>>;

/// Which palette column notes compile with, and at what prose size. `Paper`
/// is what the vanilla CLI produces without inputs at the default size
/// (white page, for `make check-vault` and exports); the app always passes
/// its own theme and its current font-size signal.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum RenderTheme {
    Paper(u16),
    Dark(u16),
    Light(u16),
}

impl RenderTheme {
    fn colour(self) -> &'static str {
        match self {
            RenderTheme::Paper(_) => "paper",
            RenderTheme::Dark(_) => "dark",
            RenderTheme::Light(_) => "light",
        }
    }

    fn size(self) -> u16 {
        match self {
            RenderTheme::Paper(size)
            | RenderTheme::Dark(size)
            | RenderTheme::Light(size) => size,
        }
    }

    /// One `Library` per (colour, size) pair the app has actually asked
    /// for, memoized behind a mutex: a `Library` is otherwise immutable, so
    /// building it once and sharing the `Arc` is exact rather than an
    /// approximation, and the map only grows across the handful of pairs a
    /// running app renders. A poisoned lock degrades to a fresh build
    /// instead of panicking — losing the memo costs a recompile, never a
    /// crash.
    fn library(self) -> Arc<LazyHash<Library>> {
        static LIBRARIES: LazyLock<LibraryMemo> =
            LazyLock::new(|| Mutex::new(HashMap::new()));
        let key = (self.colour(), self.size());
        let mut libraries =
            LIBRARIES.lock().unwrap_or_else(PoisonError::into_inner);
        libraries
            .entry(key)
            .or_insert_with(|| Arc::new(themed_library(key.0, key.1)))
            .clone()
    }
}

pub struct VaultWorld {
    root: PathBuf,
    main: FileId,
    source: Source,
    library: Arc<LazyHash<Library>>,
}

impl VaultWorld {
    pub fn new(
        root: &Path,
        note: &Path,
        text: String,
        theme: RenderTheme,
    ) -> Result<VaultWorld, VirtualizeError> {
        let vpath = VirtualPath::virtualize(root, note)?;
        let main = RootedPath::new(VirtualRoot::Project, vpath).intern();
        let source = Source::new(main, text);
        Ok(VaultWorld {
            root: root.to_path_buf(),
            main,
            source,
            library: theme.library(),
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
        self.library.as_ref()
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

/// One queued fragment compile: everything `render_svg` needs, carried to
/// the compute tier, plus the content-addressed key the outcome lands back
/// under (adr/2026-08-compute-tier-worker-seam.md). Content-addressing
/// makes a result valid for its key however late it lands — except across
/// a template change, the one compile input the key never carries, so the
/// job also rides the cache's epoch
/// (adr/2026-08-template-touch-clears-caches.md).
#[derive(Debug)]
pub struct FragmentJob {
    pub key: u64,
    pub epoch: u64,
    root: PathBuf,
    note: PathBuf,
    source: String,
    theme: RenderTheme,
}

impl FragmentJob {
    pub fn compile(&self) -> Result<String, String> {
        render_svg(&self.root, &self.note, &self.source, self.theme)
            .map_err(describe)
    }
}

/// What a fragment probe answers: the cached result, or "not yet" with the
/// job to submit — present only the first time, the in-flight set dedups
/// the repaints in between (adr/2026-08-async-caches-pending-stale.md). A
/// block's key is its own content, so a cursor move changes no key; a
/// content change does, and while the new compile is out `shelved` is the
/// last SVG the same block slot showed — a changed formula keeps its last
/// image rather than dropping to dimmed source
/// (adr/2026-09-fragments-shelve-their-last-svg-per-block.md).
#[derive(Debug)]
pub enum FragmentView {
    Ready(Result<String, String>),
    Pending {
        job: Option<FragmentJob>,
        shelved: Option<String>,
    },
}

/// Per-block SVG fragments for the hybrid editor — the successor
/// adr/2026-07-svg-cache-per-path.md predicted, decided in
/// adr/2026-07-block-segmentation-parbreak-tiling.md, then regrouped from
/// one entry per line to one entry per cursor-split region
/// (adr/2026-08-cursor-split-rendering.md), and now one entry per block
/// that needs the compiled-Typst fallback — every block whose markup CSS
/// can draw never touches this cache at all. Keyed by note path + block
/// source, so an unchanged block never recompiles; errors are cached too,
/// because a failing block would otherwise recompile on every re-render.
/// `sweep`, called at every resegmentation, drops what the current
/// generation never rendered — the mark-and-sweep bound is now the count
/// of fallback blocks in the open note, not a per-line cost ceiling. Two
/// ways in, one per compute adapter: `render` compiles on a miss in place
/// (the inline adapter), `probe` answers `Pending` and hands the compile to
/// the worker (the queued adapter). A block's key never changes while its
/// content doesn't, so a cursor move recompiles nothing and there is no
/// stale shelf left to keep fresh.
#[derive(Debug, Default)]
pub struct FragmentCache {
    entries: HashMap<u64, Result<String, String>>,
    touched: HashSet<u64>,
    inflight: HashSet<u64>,
    /// Per (note, block slot): the key of the last SVG that slot showed,
    /// whose entry `sweep` keeps even while the block is active and probes
    /// nothing — the stale-while-revalidate shelf, indexed by position
    /// because the content key is exactly what changes under an edit.
    /// Errors are never shelved, and a sweep drops the shelves of every
    /// note but the one last probed, so the memory bound is the open
    /// note's block count.
    shelves: HashMap<(PathBuf, usize), u64>,
    /// The note the last probe was for: whose shelves a sweep keeps.
    current: Option<PathBuf>,
    /// Bumped by `clear` when the template changes: the template is the one
    /// compile input outside the content-addressed key, so across its edits
    /// every cached and in-flight result is wrong for its key
    /// (adr/2026-08-template-touch-clears-caches.md).
    epoch: u64,
}

impl FragmentCache {
    pub fn render(
        &mut self,
        root: &Path,
        note: &Path,
        source: &str,
        theme: RenderTheme,
    ) -> Result<String, String> {
        let key = hash_fragment(note, source, theme);
        self.touched.insert(key);
        self.entries
            .entry(key)
            .or_insert_with(|| {
                render_svg(root, note, source, theme).map_err(describe)
            })
            .clone()
    }

    /// The queued adapter's read: never compiles, so the frame that first
    /// needs a pixel no longer pays for it (adr/2026-08-compute-tier-worker-seam.md).
    pub fn probe(
        &mut self,
        root: &Path,
        note: &Path,
        slot: usize,
        source: &str,
        theme: RenderTheme,
    ) -> FragmentView {
        let key = hash_fragment(note, source, theme);
        self.touched.insert(key);
        self.current = Some(note.to_path_buf());
        let shelf = (note.to_path_buf(), slot);
        if let Some(hit) = self.entries.get(&key) {
            if hit.is_ok() {
                self.shelves.insert(shelf, key);
            }
            return FragmentView::Ready(hit.clone());
        }
        // the slot's last good SVG stands in while the compile is out; a
        // shelf whose entry is gone (a template clear) stands in nothing
        let shelved = self
            .shelves
            .get(&shelf)
            .and_then(|old| self.entries.get(old))
            .and_then(|hit| hit.as_ref().ok())
            .cloned();
        let job = self.inflight.insert(key).then(|| FragmentJob {
            key,
            epoch: self.epoch,
            root: root.to_path_buf(),
            note: note.to_path_buf(),
            source: source.to_string(),
            theme,
        });
        FragmentView::Pending { job, shelved }
    }

    /// A worker outcome landing. A key swept while its compile was out is
    /// re-inserted here and swept again next resegmentation — harmless,
    /// content-addressing means the value is right whenever it arrives —
    /// unless the template changed since it was queued: then its epoch is
    /// old and the result is dropped whole.
    pub fn absorb(
        &mut self,
        key: u64,
        epoch: u64,
        result: Result<String, String>,
    ) {
        if epoch != self.epoch {
            return;
        }
        self.inflight.remove(&key);
        self.entries.insert(key, result);
    }

    /// Drops what the generation never rendered — except the open note's
    /// shelves, which outlive the sweeps an active block's edits cause;
    /// every other note's shelves go with the generation.
    pub fn sweep(&mut self) {
        let current = self.current.clone();
        self.shelves
            .retain(|(note, _), _| Some(note) == current.as_ref());
        let shelved: HashSet<u64> = self.shelves.values().copied().collect();
        self.entries.retain(|key, _| {
            self.touched.contains(key) || shelved.contains(key)
        });
        self.touched.clear();
    }

    /// A template change: every result, cached or in flight, compiled
    /// against a file that no longer says that. The bump dooms late
    /// outcomes; clearing in-flight lets the next probe re-queue.
    pub fn clear(&mut self) {
        self.epoch += 1;
        self.entries.clear();
        self.touched.clear();
        self.inflight.clear();
        self.shelves.clear();
        self.current = None;
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

/// One queued PDF export: the note compiled as the vanilla CLI would
/// compile it — paper palette, default size — and written beside its
/// source as `<stem>.pdf`, atomically. Runs on the compute tier like every
/// other compile (adr/2026-09-export-writes-the-pdf-beside-the-note.md).
#[derive(Debug)]
pub struct ExportJob {
    /// Vault-relative, like a body's key.
    pub note: PathBuf,
    root: PathBuf,
}

impl ExportJob {
    pub fn new(root: &Path, note: &Path) -> ExportJob {
        ExportJob {
            note: note.to_path_buf(),
            root: root.to_path_buf(),
        }
    }

    /// Answers the PDF's vault-relative path. Every step reports through
    /// one prefix so the status line names the stage that failed: the
    /// read, the compile, the PDF encoding, or the write. The error
    /// mappers are named functions rather than closures so a stage that
    /// cannot be made to fail from a note (the encoding) leaves no dead
    /// arm behind.
    pub fn export(&self) -> Result<PathBuf, String> {
        let file = self.root.join(&self.note);
        let target = self.note.with_extension("pdf");
        let bytes = std::fs::read_to_string(&file)
            .map_err(export_io_error)
            .and_then(|text| {
                compile_document(
                    &self.root,
                    &file,
                    &text,
                    RenderTheme::Paper(DEFAULT_SIZE),
                )
                .map_err(export_render_error)
            })
            .and_then(|document| {
                typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
                    .map_err(diagnostics)
                    .map_err(export_render_error)
            })?;
        crate::persist::write_atomic_bytes(&self.root.join(&target), &bytes)
            .map_err(export_io_error)?;
        Ok(target)
    }
}

fn export_io_error(error: std::io::Error) -> String {
    format!("export: {error}")
}

fn export_render_error(error: RenderError) -> String {
    format!("export: {}", describe(error))
}

pub fn render_svg(
    root: &Path,
    note: &Path,
    text: &str,
    theme: RenderTheme,
) -> Result<String, RenderError> {
    let doc = compile_document(root, note, text, theme)?;
    Ok(typst_svg::svg_merged(
        &doc,
        &SvgOptions::default(),
        Abs::pt(0.0),
    ))
}

/// The one compile every output shares — the SVG the app draws and the
/// PDF an export writes come from the same document.
fn compile_document(
    root: &Path,
    note: &Path,
    text: &str,
    theme: RenderTheme,
) -> Result<PagedDocument, RenderError> {
    let world = VaultWorld::new(root, note, text.to_string(), theme)?;
    typst::compile::<PagedDocument>(&world)
        .output
        .map_err(diagnostics)
}

/// Typst's diagnostics as the plain messages the status line shows.
fn diagnostics(
    errors: impl IntoIterator<Item = typst::diag::SourceDiagnostic>,
) -> RenderError {
    RenderError::Compile(
        errors.into_iter().map(|e| e.message.to_string()).collect(),
    )
}

/// The path joins the hash because fragments compile under their note's
/// path: two notes could hold byte-identical blocks whose relative
/// resolution differs; the theme joins it because the same source renders
/// differently per palette column. Stable only within this process:
/// `DefaultHasher` may change across Rust releases, so these hashes must
/// never be persisted to `.index/`.
fn hash_fragment(note: &Path, source: &str, theme: RenderTheme) -> u64 {
    let mut hasher = DefaultHasher::new();
    note.hash(&mut hasher);
    source.hash(&mut hasher);
    theme.hash(&mut hasher);
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

    // -- the probe interface: never compiles, queues exactly once ------------

    #[test]
    fn a_fragment_probe_queues_once_then_serves_what_lands() {
        let mut cache = FragmentCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let FragmentView::Pending { job: Some(job), .. } = cache.probe(
            root,
            note,
            0,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
        ) else {
            panic!("the first probe hands the job over");
        };
        let FragmentView::Pending { job: None, .. } = cache.probe(
            root,
            note,
            0,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
        ) else {
            panic!("a repaint mid-flight queues nothing");
        };
        cache.absorb(job.key, job.epoch, Ok("<svg/>".to_string()));
        let FragmentView::Ready(Ok(svg)) = cache.probe(
            root,
            note,
            0,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
        ) else {
            panic!("the landed outcome answers the next probe");
        };
        assert_eq!(svg, "<svg/>");
    }

    /// Probes slot 0 of one note with `source`, in the dark theme.
    fn probe_slot(cache: &mut FragmentCache, source: &str) -> FragmentView {
        cache.probe(
            Path::new("/vault"),
            Path::new("permanent/a.typ"),
            0,
            source,
            RenderTheme::Dark(DEFAULT_SIZE),
        )
    }

    /// Lands `svg` for the job `view` carries.
    fn land(cache: &mut FragmentCache, view: FragmentView, svg: &str) {
        let FragmentView::Pending { job: Some(job), .. } = view else {
            panic!("a fresh key hands its job over");
        };
        cache.absorb(job.key, job.epoch, Ok(svg.to_string()));
    }

    #[test]
    fn a_changed_block_keeps_its_last_svg_while_the_new_compile_is_out() {
        let mut cache = FragmentCache::default();
        let first = probe_slot(&mut cache, "$ a $");
        land(&mut cache, first, "<a/>");
        assert!(matches!(
            probe_slot(&mut cache, "$ a $"),
            FragmentView::Ready(Ok(_))
        ));

        // the formula changed: its new key is out, its old image stands in
        let FragmentView::Pending {
            job: Some(job),
            shelved: Some(shelved),
        } = probe_slot(&mut cache, "$ a + b $")
        else {
            panic!("a changed slot pends with its last svg shelved");
        };
        assert_eq!(shelved, "<a/>");
        // a sweep between the change and the landing keeps the shelf
        cache.sweep();
        let FragmentView::Pending {
            job: None,
            shelved: Some(kept),
        } = probe_slot(&mut cache, "$ a + b $")
        else {
            panic!("mid-flight, the shelf survives the sweep");
        };
        assert_eq!(kept, "<a/>");

        // the fresh compile lands and is the next shelf
        cache.absorb(job.key, job.epoch, Ok("<ab/>".to_string()));
        assert!(matches!(
            probe_slot(&mut cache, "$ a + b $"),
            FragmentView::Ready(Ok(_))
        ));
        cache.sweep();
        let FragmentView::Pending {
            shelved: Some(next),
            ..
        } = probe_slot(&mut cache, "$ a + b + c $")
        else {
            panic!("the landed svg is what the slot shelves now");
        };
        assert_eq!(next, "<ab/>");
    }

    #[test]
    fn a_shelf_outlives_the_sweeps_of_an_edit_and_dies_with_the_note() {
        let mut cache = FragmentCache::default();
        let first = probe_slot(&mut cache, "$ a $");
        land(&mut cache, first, "<a/>");
        probe_slot(&mut cache, "$ a $");
        // the block is active: generations pass in which it probes nothing
        cache.sweep();
        cache.sweep();
        let FragmentView::Pending {
            shelved: Some(kept),
            ..
        } = probe_slot(&mut cache, "$ a + b $")
        else {
            panic!("the shelf survived the edit's sweeps");
        };
        assert_eq!(kept, "<a/>");

        // another note takes the screen: the first note's shelf goes with
        // the next sweep, so a return to it shelves nothing
        let other = cache.probe(
            Path::new("/vault"),
            Path::new("permanent/b.typ"),
            0,
            "= b",
            RenderTheme::Dark(DEFAULT_SIZE),
        );
        land(&mut cache, other, "<b/>");
        cache.sweep();
        let FragmentView::Pending { shelved: None, .. } =
            probe_slot(&mut cache, "$ a + b + c $")
        else {
            panic!("a shelf never outlives its note's visit");
        };
    }

    #[test]
    fn an_error_is_never_shelved_and_a_swept_shelf_stands_in_nothing() {
        let mut cache = FragmentCache::default();
        let first = probe_slot(&mut cache, "$ a $");
        land(&mut cache, first, "<a/>");
        probe_slot(&mut cache, "$ a $");

        let FragmentView::Pending { job: Some(job), .. } =
            probe_slot(&mut cache, "$ a + $")
        else {
            panic!("the broken formula pends");
        };
        cache.absorb(job.key, job.epoch, Err("unclosed".to_string()));
        assert!(matches!(
            probe_slot(&mut cache, "$ a + $"),
            FragmentView::Ready(Err(_))
        ));
        // the error is drawn, and the good image before it is still the
        // shelf: the next change shows the last thing that rendered
        cache.sweep();
        probe_slot(&mut cache, "$ a + $");
        cache.sweep();
        let FragmentView::Pending {
            shelved: Some(last_good),
            ..
        } = probe_slot(&mut cache, "$ a + b $")
        else {
            panic!("an error is never a shelf; the image before it is");
        };
        assert_eq!(last_good, "<a/>");

        // a template clear forgets every shelf too
        let fresh = probe_slot(&mut cache, "$ b $");
        land(&mut cache, fresh, "<b/>");
        probe_slot(&mut cache, "$ b $");
        cache.clear();
        let FragmentView::Pending { shelved: None, .. } =
            probe_slot(&mut cache, "$ b + c $")
        else {
            panic!("a cleared cache shelves nothing");
        };
    }

    #[test]
    fn a_cleared_fragment_cache_drops_the_outcomes_it_doomed() {
        // the template changed while the compile was out: same key, wrong
        // pixels — the epoch bump keeps the late landing out
        let mut cache = FragmentCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let FragmentView::Pending {
            job: Some(doomed), ..
        } = cache.probe(
            root,
            note,
            0,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
        )
        else {
            panic!("the first probe hands the job over");
        };
        cache.clear();
        cache.absorb(doomed.key, doomed.epoch, Ok("<old/>".to_string()));
        let FragmentView::Pending {
            job: Some(fresh), ..
        } = cache.probe(
            root,
            note,
            0,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
        )
        else {
            panic!("nothing landed, so the cleared cache re-queues");
        };
        assert_eq!(fresh.key, doomed.key, "the key is content-addressed");
        assert!(fresh.epoch > doomed.epoch, "the clear moved the epoch");
    }

    #[test]
    fn a_cleared_fragment_cache_forgets_what_was_ready() {
        let mut cache = FragmentCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let FragmentView::Pending { job: Some(job), .. } = cache.probe(
            root,
            note,
            0,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
        ) else {
            panic!("the first probe hands the job over");
        };
        cache.absorb(job.key, job.epoch, Ok("<svg/>".to_string()));
        cache.clear();
        let FragmentView::Pending { job: Some(_), .. } = cache.probe(
            root,
            note,
            0,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
        ) else {
            panic!(
                "the ready entry compiled against the old template, and a \
                 template touch keeps nothing cached either \
                 (adr/2026-08-template-touch-clears-caches.md)"
            );
        };
    }

    #[test]
    fn a_fragment_job_compiles_like_the_synchronous_render() {
        let vault =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
        let note = vault.join("permanent/zettelkasten.typ");
        let FragmentView::Pending { job: Some(job), .. } =
            FragmentCache::default().probe(
                &vault,
                &note,
                0,
                "= titre\n",
                RenderTheme::Paper(DEFAULT_SIZE),
            )
        else {
            panic!("a fresh cache queues the compile");
        };
        let inline = FragmentCache::default().render(
            &vault,
            &note,
            "= titre\n",
            RenderTheme::Paper(DEFAULT_SIZE),
        );
        assert_eq!(job.compile(), inline);
        assert!(inline.expect("a heading compiles").contains("<svg"));
    }

    fn fixture_world() -> VaultWorld {
        let vault =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
        let note = vault.join("permanent/zettelkasten.typ");
        // the text is irrelevant to `read`; an empty buffer keeps the
        // fixture free of a disk read
        VaultWorld::new(
            &vault,
            &note,
            String::new(),
            RenderTheme::Paper(DEFAULT_SIZE),
        )
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
