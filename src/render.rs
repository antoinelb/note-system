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

/// One of the two compiled regions the cursor split makes
/// (adr/2026-08-cursor-split-rendering.md) — the discriminator
/// `FragmentCache`'s stale shelf is keyed by, since the content-addressed
/// key changes on every cursor line move and so can never itself name "the
/// previous compile for this side"
/// (adr/2026-08-region-recompile-keeps-the-stale-svg.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Above,
    Below,
}

/// What a fragment probe answers: the cached result, or "not yet" with the
/// last good SVG for this side to keep showing (stale-while-revalidate,
/// `BodyView`'s twin) and the job to submit — present only the first time,
/// the in-flight set dedups the repaints in between
/// (adr/2026-08-async-caches-pending-stale.md,
/// adr/2026-08-region-recompile-keeps-the-stale-svg.md).
#[derive(Debug)]
pub enum FragmentView {
    Ready(Result<String, String>),
    Pending {
        stale: Option<String>,
        job: Option<FragmentJob>,
    },
}

/// Region SVG fragments for the hybrid editor — the successor
/// adr/2026-07-svg-cache-per-path.md predicted, decided in
/// adr/2026-07-block-segmentation-parbreak-tiling.md, then regrouped from
/// one entry per line to one entry per cursor-split region
/// (adr/2026-08-cursor-split-rendering.md). Keyed by note path + fragment
/// source, so an unchanged region never recompiles; errors are cached too,
/// because a failing region would otherwise recompile on every re-render.
/// `sweep`, called at every resegmentation, drops what the current
/// generation never rendered — with at most two regions live per open
/// note (above and below the active line), the mark-and-sweep bound is no
/// longer a per-line cost ceiling, just a two-entry cache. Two ways in,
/// one per compute adapter: `render` compiles on a miss in place (the
/// inline adapter), `probe` answers `Pending` and hands the compile to the
/// worker (the queued adapter). Both keep the per-side `stale` shelf fresh
/// on every hit, so a cursor move that changes a region's content-hash key
/// still has the previous compile to show while the new one is out
/// (adr/2026-08-region-recompile-keeps-the-stale-svg.md).
#[derive(Debug, Default)]
pub struct FragmentCache {
    entries: HashMap<u64, Result<String, String>>,
    touched: HashSet<u64>,
    inflight: HashSet<u64>,
    /// The last good SVG per region side, kept across a content-hash miss
    /// so a cursor moving one line does not drop straight to raw dimmed
    /// source while the replacement compiles — `BodyCache`'s `stale` shelf,
    /// keyed by side instead of by path since a region's key is content,
    /// not identity (adr/2026-08-region-recompile-keeps-the-stale-svg.md).
    stale: HashMap<Side, String>,
    /// Which (note, theme) the shelf's SVGs belong to. The shelf survives
    /// content-hash misses on purpose, but a miss caused by switching notes
    /// or themes would otherwise serve the *previous* note's prose (or the
    /// old theme's pixels) undimmed while the first compile is out — so
    /// crossing either boundary empties the shelf instead
    /// (adr/2026-08-region-recompile-keeps-the-stale-svg.md).
    stale_for: Option<(PathBuf, RenderTheme)>,
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
        side: Side,
    ) -> Result<String, String> {
        self.freshen_shelf(note, theme);
        let key = hash_fragment(note, source, theme);
        self.touched.insert(key);
        let result = self
            .entries
            .entry(key)
            .or_insert_with(|| {
                render_svg(root, note, source, theme).map_err(describe)
            })
            .clone();
        if let Ok(svg) = &result {
            self.stale.insert(side, svg.clone());
        }
        result
    }

    /// The queued adapter's read: never compiles, so the frame that first
    /// needs a pixel no longer pays for it (adr/2026-08-compute-tier-worker-seam.md).
    pub fn probe(
        &mut self,
        root: &Path,
        note: &Path,
        source: &str,
        theme: RenderTheme,
        side: Side,
    ) -> FragmentView {
        self.freshen_shelf(note, theme);
        let key = hash_fragment(note, source, theme);
        self.touched.insert(key);
        if let Some(hit) = self.entries.get(&key) {
            if let Ok(svg) = hit {
                self.stale.insert(side, svg.clone());
            }
            return FragmentView::Ready(hit.clone());
        }
        let stale = self.stale.get(&side).cloned();
        let job = self.inflight.insert(key).then(|| FragmentJob {
            key,
            epoch: self.epoch,
            root: root.to_path_buf(),
            note: note.to_path_buf(),
            source: source.to_string(),
            theme,
        });
        FragmentView::Pending { stale, job }
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

    pub fn sweep(&mut self) {
        self.entries.retain(|key, _| self.touched.contains(key));
        self.touched.clear();
    }

    /// A template change: every result, cached or in flight, compiled
    /// against a file that no longer says that. The bump dooms late
    /// outcomes; clearing in-flight lets the next probe re-queue. `stale`
    /// clears too — fragments show no stale pixels across a template touch,
    /// unlike bodies, which keep theirs
    /// (adr/2026-08-template-touch-clears-caches.md): a moving cursor is
    /// still showing a compile of the *same* template, but a template edit
    /// means every held SVG, fresh or stale, is wrong for it.
    pub fn clear(&mut self) {
        self.epoch += 1;
        self.entries.clear();
        self.touched.clear();
        self.inflight.clear();
        self.stale.clear();
        self.stale_for = None;
    }

    /// The shelf's identity check, run before every read: same (note,
    /// theme) keeps the shelf across content-hash misses, a different one
    /// empties it — stale is only ever this note under this theme.
    fn freshen_shelf(&mut self, note: &Path, theme: RenderTheme) {
        let held =
            self.stale_for
                .as_ref()
                .is_some_and(|(for_note, for_theme)| {
                    for_note == note && *for_theme == theme
                });
        if !held {
            self.stale.clear();
            self.stale_for = Some((note.to_path_buf(), theme));
        }
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

/// One queued body compile — `FragmentJob`'s sibling. Bodies are keyed by
/// path, not content, so an outcome can be stale: it carries the epoch it
/// was queued under, and `absorb` drops it if an invalidation has bumped
/// the epoch since (adr/2026-08-async-caches-pending-stale.md).
#[derive(Debug)]
pub struct BodyJob {
    pub note: PathBuf,
    pub theme: RenderTheme,
    pub epoch: u64,
    root: PathBuf,
}

impl BodyJob {
    pub fn compile(&self) -> Result<String, String> {
        compile_body(&self.root, &self.note, self.theme)
    }
}

/// What a body probe answers: the cached result, or "not yet" with the
/// last good SVG to keep showing (stale-while-revalidate) and the job to
/// submit — present only the first time, like a fragment's.
#[derive(Debug)]
pub enum BodyView {
    Ready(Result<String, String>),
    Pending {
        stale: Option<String>,
        job: Option<BodyJob>,
    },
}

/// Whole-note SVGs for the table's body zoom — `FragmentCache`'s sibling
/// with the opposite lifecycle: the table shows many notes at once where
/// the sheet shows one, so entries live until the watcher says their file
/// changed rather than being swept per open note
/// (adr/2026-08-body-cache-per-note-svg.md). Reads the file itself on a
/// miss; errors — unreadable or uncompilable — are cached like the
/// fragment cache's. In-process only, like every render hash. The same
/// two ways in as the fragment cache: `render` compiles in place, `probe`
/// queues — and because the key is a path whose content can change under
/// an in-flight compile, invalidation bumps an epoch that dooms every
/// outcome queued before it.
#[derive(Debug, Default)]
pub struct BodyCache {
    entries: HashMap<(PathBuf, RenderTheme), Result<String, String>>,
    /// The last good SVG per key, shown while a recompile is pending —
    /// never an error: seeing those is the point of `Ready(Err)`.
    stale: HashMap<(PathBuf, RenderTheme), String>,
    inflight: HashSet<(PathBuf, RenderTheme)>,
    epoch: u64,
}

impl BodyCache {
    pub fn render(
        &mut self,
        root: &Path,
        note: &Path,
        theme: RenderTheme,
    ) -> Result<String, String> {
        let key = (note.to_path_buf(), theme);
        if let Some(cached) = self.entries.get(&key) {
            return cached.clone();
        }
        let rendered = compile_body(root, note, theme);
        self.entries.insert(key, rendered.clone());
        rendered
    }

    /// The queued adapter's read — `FragmentCache::probe`'s twin.
    pub fn probe(
        &mut self,
        root: &Path,
        note: &Path,
        theme: RenderTheme,
    ) -> BodyView {
        let key = (note.to_path_buf(), theme);
        if let Some(hit) = self.entries.get(&key) {
            return BodyView::Ready(hit.clone());
        }
        let stale = self.stale.get(&key).cloned();
        let job = self.inflight.insert(key).then(|| BodyJob {
            note: note.to_path_buf(),
            theme,
            epoch: self.epoch,
            root: root.to_path_buf(),
        });
        BodyView::Pending { stale, job }
    }

    /// A worker outcome landing — dropped whole if an invalidation bumped
    /// the epoch after it was queued: it compiled a file that has since
    /// changed, and the bump already cleared its in-flight slot, so the
    /// next probe re-queues against the current epoch.
    pub fn absorb(
        &mut self,
        note: PathBuf,
        theme: RenderTheme,
        epoch: u64,
        result: Result<String, String>,
    ) {
        if epoch != self.epoch {
            return;
        }
        self.inflight.remove(&(note.clone(), theme));
        self.entries.insert((note, theme), result);
    }

    /// The watcher's per-path invalidation: every cached (theme, size)
    /// column for this path drops — the file changed for all of them alike.
    pub fn invalidate(&mut self, note: &Path) {
        self.expire(|(path, _)| path == note);
    }

    /// A rescan's blunt answer: everything may have changed.
    pub fn clear(&mut self) {
        self.expire(|_| true);
    }

    /// Expired entries move their last good SVG to the stale shelf; errors
    /// just drop. The epoch bump dooms every in-flight outcome, so the
    /// whole in-flight set clears and doomed keys re-queue at their next
    /// probe.
    // ponytail: one global epoch — a bump discards unrelated in-flight
    // compiles too, which re-queue; per-key epochs if that ever thrashes
    fn expire(&mut self, expired: impl Fn(&(PathBuf, RenderTheme)) -> bool) {
        self.epoch += 1;
        self.inflight.clear();
        let dead: Vec<_> = self
            .entries
            .keys()
            .filter(|key| expired(key))
            .cloned()
            .collect();
        for key in dead {
            let last_good = self.entries.remove(&key).and_then(Result::ok);
            if let Some(svg) = last_good {
                self.stale.insert(key, svg);
            }
        }
    }
}

/// The one body compile both adapters share: read the file, render it. The
/// world wants the absolute path; the key stays vault-relative, the shape
/// the index and the watcher both speak.
fn compile_body(
    root: &Path,
    note: &Path,
    theme: RenderTheme,
) -> Result<String, String> {
    let file = root.join(note);
    std::fs::read_to_string(&file)
        .map_err(|error| format!("body: {error}"))
        .and_then(|text| {
            render_svg(root, &file, &text, theme).map_err(describe)
        })
}

pub fn render_svg(
    root: &Path,
    note: &Path,
    text: &str,
    theme: RenderTheme,
) -> Result<String, RenderError> {
    let world = VaultWorld::new(root, note, text.to_string(), theme)?;

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
        let FragmentView::Pending {
            stale: None,
            job: Some(job),
        } = cache.probe(
            root,
            note,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
            Side::Above,
        )
        else {
            panic!("the first probe has no stale SVG and hands the job over");
        };
        let FragmentView::Pending { job: None, .. } = cache.probe(
            root,
            note,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
            Side::Above,
        ) else {
            panic!("a repaint mid-flight queues nothing");
        };
        cache.absorb(job.key, job.epoch, Ok("<svg/>".to_string()));
        let FragmentView::Ready(Ok(svg)) = cache.probe(
            root,
            note,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
            Side::Above,
        ) else {
            panic!("the landed outcome answers the next probe");
        };
        assert_eq!(svg, "<svg/>");
    }

    /// The cursor split's own new problem: the region's key is content, not
    /// identity, so it changes on every line the cursor crosses. The
    /// previous compile for the same side has to stand in until the fresh
    /// one lands, or every `j`/`k` flashes to raw dimmed source
    /// (adr/2026-08-region-recompile-keeps-the-stale-svg.md).
    #[test]
    fn a_region_recompile_serves_its_own_sides_last_good_svg() {
        let mut cache = FragmentCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let theme = RenderTheme::Dark(DEFAULT_SIZE);

        let FragmentView::Pending {
            stale: None,
            job: Some(job),
        } = cache.probe(root, note, "= a", theme, Side::Above)
        else {
            panic!("the first probe for a side has nothing stale to show");
        };
        cache.absorb(job.key, job.epoch, Ok("<a/>".to_string()));
        // a repaint of the same content is a `Ready` hit, which is also
        // where the side's shelf gets its first entry
        let FragmentView::Ready(Ok(svg)) =
            cache.probe(root, note, "= a", theme, Side::Above)
        else {
            panic!("the landed outcome answers the next probe");
        };
        assert_eq!(svg, "<a/>");

        // the cursor moves one line: the region grows by "b", so its
        // content-addressed key is new — a miss on the cache proper, but not
        // on the side's own shelf
        let FragmentView::Pending {
            stale: Some(stale),
            job: Some(moved),
        } = cache.probe(root, note, "= a\n\nb", theme, Side::Above)
        else {
            panic!("a moved cursor still has the previous compile to show");
        };
        assert_eq!(stale, "<a/>");
        cache.absorb(moved.key, moved.epoch, Ok("<ab/>".to_string()));
        let FragmentView::Ready(Ok(svg)) =
            cache.probe(root, note, "= a\n\nb", theme, Side::Above)
        else {
            panic!("the recompile lands");
        };
        assert_eq!(svg, "<ab/>");

        // the other side of the same note has never compiled: its shelf is
        // its own, not the above region's
        let FragmentView::Pending { stale: None, .. } =
            cache.probe(root, note, "= z", theme, Side::Below)
        else {
            panic!("a side starts with no stale of its own");
        };
    }

    /// The shelf's identity: staleness only ever means "this note under
    /// this theme, a moment ago". Crossing a note or theme boundary must
    /// empty it — a shelf hit there would show the previous note's prose
    /// (or the old theme's pixels) undimmed until the first compile lands
    /// (adr/2026-08-region-recompile-keeps-the-stale-svg.md).
    #[test]
    fn switching_the_note_or_the_theme_empties_the_shelf() {
        let mut cache = FragmentCache::default();
        let root = Path::new("/vault");
        let note_a = Path::new("permanent/a.typ");
        let dark = RenderTheme::Dark(DEFAULT_SIZE);

        let FragmentView::Pending { job: Some(job), .. } =
            cache.probe(root, note_a, "= a", dark, Side::Above)
        else {
            panic!("the first probe queues the compile");
        };
        cache.absorb(job.key, job.epoch, Ok("<a/>".to_string()));
        let FragmentView::Ready(Ok(_)) =
            cache.probe(root, note_a, "= a", dark, Side::Above)
        else {
            panic!("the landed outcome fills the shelf");
        };

        // another note: its first probe misses, and note a's SVG must not
        // stand in for it
        let note_b = Path::new("permanent/b.typ");
        let FragmentView::Pending { stale: None, .. } =
            cache.probe(root, note_b, "= b", dark, Side::Above)
        else {
            panic!("a shelf never crosses notes");
        };

        // back on note a: the content-addressed entry still answers — only
        // the shelf reset — and the hit refills the shelf for note a
        let FragmentView::Ready(Ok(_)) =
            cache.probe(root, note_a, "= a", dark, Side::Above)
        else {
            panic!("the entry survived the note switch; only stale reset");
        };
        let light = RenderTheme::Light(DEFAULT_SIZE);
        let FragmentView::Pending { stale: None, .. } =
            cache.probe(root, note_a, "= a", light, Side::Above)
        else {
            panic!("a shelf never crosses themes");
        };
    }

    /// An error is nothing to keep showing — `render`'s inline path never
    /// shelves one, and neither does a `probe` hit that finds one already
    /// cached (`BodyCache`'s `an_expired_error_drops_instead_of_shelving`
    /// makes the same call for bodies).
    #[test]
    fn a_failed_region_is_cached_but_never_shelved_as_stale() {
        let mut cache = FragmentCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let theme = RenderTheme::Dark(DEFAULT_SIZE);
        // an unclosed paren: a real Typst compile error, not a fabricated one
        let broken = "#let x = (";

        let rendered = cache.render(root, note, broken, theme, Side::Above);
        assert!(rendered.is_err(), "{rendered:?}");
        let FragmentView::Ready(Err(_)) =
            cache.probe(root, note, broken, theme, Side::Above)
        else {
            panic!("the cached error answers the next probe");
        };
        let FragmentView::Pending { stale: None, .. } =
            cache.probe(root, note, "= a", theme, Side::Above)
        else {
            panic!("an error left nothing on the shelf to show instead");
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
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
            Side::Above,
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
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
            Side::Above,
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
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
            Side::Above,
        ) else {
            panic!("the first probe hands the job over");
        };
        cache.absorb(job.key, job.epoch, Ok("<svg/>".to_string()));
        cache.clear();
        let FragmentView::Pending {
            stale: None,
            job: Some(_),
        } = cache.probe(
            root,
            note,
            "= titre",
            RenderTheme::Dark(DEFAULT_SIZE),
            Side::Above,
        )
        else {
            panic!(
                "the ready entry compiled against the old template, and a \
                 template touch keeps no stale fragment either \
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
                "= titre\n",
                RenderTheme::Paper(DEFAULT_SIZE),
                Side::Above,
            )
        else {
            panic!("a fresh cache queues the compile");
        };
        let inline = FragmentCache::default().render(
            &vault,
            &note,
            "= titre\n",
            RenderTheme::Paper(DEFAULT_SIZE),
            Side::Above,
        );
        assert_eq!(job.compile(), inline);
        assert!(inline.expect("a heading compiles").contains("<svg"));
    }

    #[test]
    fn a_body_probe_queues_once_and_a_late_outcome_lands() {
        let mut cache = BodyCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let BodyView::Pending {
            stale: None,
            job: Some(job),
        } = cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("a first probe has no stale SVG and hands the job over");
        };
        let BodyView::Pending { job: None, .. } =
            cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("a repaint mid-flight queues nothing");
        };
        cache.absorb(
            job.note.clone(),
            job.theme,
            job.epoch,
            Ok("<svg/>".to_string()),
        );
        let BodyView::Ready(Ok(svg)) =
            cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("the landed outcome answers the next probe");
        };
        assert_eq!(svg, "<svg/>");
    }

    #[test]
    fn an_invalidated_body_serves_its_last_good_svg_while_pending() {
        let mut cache = BodyCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let BodyView::Pending { job: Some(job), .. } =
            cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("the first probe queues");
        };
        cache.absorb(job.note, job.theme, job.epoch, Ok("<old/>".to_string()));

        cache.invalidate(note);
        let BodyView::Pending {
            stale: Some(stale),
            job: Some(fresh),
        } = cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("the invalidated body re-queues with its last good SVG");
        };
        assert_eq!(stale, "<old/>");
        cache.absorb(
            fresh.note,
            fresh.theme,
            fresh.epoch,
            Ok("<new/>".into()),
        );
        let BodyView::Ready(Ok(svg)) =
            cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("the recompile lands");
        };
        assert_eq!(svg, "<new/>");
    }

    #[test]
    fn an_outcome_from_before_the_invalidation_is_dropped() {
        let mut cache = BodyCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let BodyView::Pending {
            job: Some(doomed), ..
        } = cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("the first probe queues");
        };
        cache.invalidate(note);
        cache.absorb(
            doomed.note,
            doomed.theme,
            doomed.epoch,
            Ok("<compiled-from-the-old-file/>".to_string()),
        );
        let BodyView::Pending { .. } =
            cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("the doomed outcome never lands; the probe re-queues");
        };
    }

    #[test]
    fn an_expired_error_drops_instead_of_shelving() {
        let mut cache = BodyCache::default();
        let (root, note) = (Path::new("/vault"), Path::new("permanent/a.typ"));
        let BodyView::Pending { job: Some(job), .. } =
            cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("the first probe queues");
        };
        cache.absorb(job.note, job.theme, job.epoch, Err("broke".to_string()));
        cache.clear();
        let BodyView::Pending { stale: None, .. } =
            cache.probe(root, note, RenderTheme::Dark(DEFAULT_SIZE))
        else {
            panic!("an error is nothing to keep showing");
        };
    }

    #[test]
    fn a_body_job_compiles_what_the_synchronous_path_would() {
        let vault =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault");
        let note = Path::new("permanent/zettelkasten.typ");
        let BodyView::Pending { job: Some(job), .. } = BodyCache::default()
            .probe(&vault, note, RenderTheme::Paper(DEFAULT_SIZE))
        else {
            panic!("a fresh cache queues the compile");
        };
        let inline = BodyCache::default().render(
            &vault,
            note,
            RenderTheme::Paper(DEFAULT_SIZE),
        );
        assert_eq!(job.compile(), inline);
        assert!(inline.expect("the fixture note compiles").contains("<svg"));
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
