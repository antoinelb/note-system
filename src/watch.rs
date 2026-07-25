use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::mpsc::Receiver,
    time::Duration,
};

use notify::RecursiveMode;
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache,
    notify::{
        self, EventKind, RecommendedWatcher,
        event::{ModifyKind, RenameMode},
    },
};

use crate::{
    domain::Note,
    index::{Index, IndexError},
    parse::parse_note,
};
use crate::{domain::NoteCategory, index::scan_vault};

const QUIET: Duration = Duration::from_millis(200);

#[derive(Debug, PartialEq, Eq)]
pub enum VaultChange {
    Touched {
        category: NoteCategory,
        path: PathBuf,
    },
    Removed(PathBuf),
    Rescan,
}

pub struct VaultWatcher {
    /// Dropping the debouncer stops its thread, so it is owned here even
    /// though nothing ever reads it.
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    pub changes: Receiver<Vec<VaultChange>>,
}

impl VaultWatcher {
    pub fn start(root: &Path) -> Result<VaultWatcher, notify::Error> {
        let (sender, changes) = std::sync::mpsc::channel();
        let watched = root.to_path_buf();
        let mut debouncer = notify_debouncer_full::new_debouncer(
            QUIET,
            faults::tick_rate(),
            move |result| {
                let batch = classify(&watched, result);
                if !batch.is_empty() {
                    let _ = sender.send(batch);
                }
            },
        )?;
        debouncer.watch(root, RecursiveMode::Recursive)?;
        Ok(VaultWatcher {
            _debouncer: debouncer,
            changes,
        })
    }
}

pub fn apply(
    index: &mut Index,
    root: &Path,
    changes: &[VaultChange],
) -> Result<(), IndexError> {
    for change in changes {
        match change {
            VaultChange::Rescan => return index.rebuild(&scan_vault(root)?),
            VaultChange::Touched { category, path } => {
                touch(index, root, *category, path)?
            }
            VaultChange::Removed(path) => index.remove_note(path)?,
        }
    }
    Ok(())
}

fn classify(root: &Path, result: DebounceEventResult) -> Vec<VaultChange> {
    match result {
        Ok(events) => events
            .into_iter()
            .flat_map(|event| changes_of(root, &event))
            .collect(),
        Err(_) => vec![VaultChange::Rescan],
    }
}

fn changes_of(root: &Path, event: &DebouncedEvent) -> Vec<VaultChange> {
    if event.need_rescan() {
        return vec![VaultChange::Rescan];
    }
    let note_paths = event
        .event
        .paths
        .iter()
        .filter_map(|path| note_path(root, path))
        .collect::<Vec<_>>();

    if note_paths.is_empty() {
        return Vec::new();
    }

    match event.event.kind {
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_)) => {
            Vec::new()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let first_change = event
                .paths
                .first()
                .and_then(|path| note_path(root, path))
                .map(|(_, path)| VaultChange::Removed(path));
            let second_change = event
                .paths
                .get(1)
                .and_then(|path| note_path(root, path))
                .map(|(category, path)| VaultChange::Touched {
                    category,
                    path,
                });
            // an Option is an iterator of zero or one, so either half can
            // fall outside the vault without a case of its own
            first_change.into_iter().chain(second_change).collect()
        }
        EventKind::Remove(_)
        | EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            removed(note_paths)
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            touched(note_paths)
        }
        EventKind::Modify(ModifyKind::Name(_)) => vec![VaultChange::Rescan],
        EventKind::Create(_) | EventKind::Modify(_) => touched(note_paths),
        EventKind::Any | EventKind::Other => vec![VaultChange::Rescan],
    }
}

fn note_path(root: &Path, path: &Path) -> Option<(NoteCategory, PathBuf)> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.extension() != Some(OsStr::new("typ")) {
        return None;
    }
    let category = NoteCategory::from_dir(
        relative.parent().unwrap_or(Path::new("")).to_str()?,
    )?;
    Some((category, relative.to_path_buf()))
}

fn removed(note_paths: Vec<(NoteCategory, PathBuf)>) -> Vec<VaultChange> {
    note_paths
        .into_iter()
        .map(|(_, path)| VaultChange::Removed(path.to_path_buf()))
        .collect()
}

fn touched(note_paths: Vec<(NoteCategory, PathBuf)>) -> Vec<VaultChange> {
    note_paths
        .into_iter()
        .map(|(category, path)| VaultChange::Touched {
            category,
            path: path.to_path_buf(),
        })
        .collect()
}

fn touch(
    index: &mut Index,
    root: &Path,
    category: NoteCategory,
    path: &Path,
) -> Result<(), IndexError> {
    let source = match std::fs::read_to_string(root.join(path)) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return index.remove_note(path);
        }
        Err(error) => return Err(error.into()),
    };
    let parsed_note = parse_note(&source);
    let note = Note {
        path: path.to_path_buf(),
        category,
        meta: parsed_note.meta,
        links: parsed_note.links,
    };
    index.update_note(&note)
}

/// Fault injection for the one error path that no real system state reaches.
///
/// Outside `cfg(test)` `tick_rate` is the constant `None` the debouncer is
/// meant to get, so the shipped call is the one the test exercises. Excluded
/// from coverage for the same reason as `index::faults`: it is scaffolding,
/// and measuring it would only measure whichever arm this build compiled.
#[cfg_attr(coverage_nightly, coverage(off))]
mod faults {
    use std::time::Duration;

    #[cfg(not(test))]
    pub(super) fn tick_rate() -> Option<Duration> {
        None
    }

    #[cfg(test)]
    pub(super) use armed::*;

    #[cfg(test)]
    mod armed {
        use super::*;
        use std::cell::Cell;

        thread_local! {
            static ARMED: Cell<bool> = const { Cell::new(false) };
        }

        /// Arms the fault until the returned guard drops, so a panicking test
        /// cannot leak it into the next test on the same thread.
        pub(in crate::watch) fn arm() -> Guard {
            ARMED.with(|armed| armed.set(true));
            Guard
        }

        pub(in crate::watch) struct Guard;

        impl Drop for Guard {
            fn drop(&mut self) {
                ARMED.with(|armed| armed.set(false));
            }
        }

        /// A tick rate longer than the debounce window is rejected by
        /// `new_debouncer_opt` before it spawns its thread or creates the
        /// watcher (notify-debouncer-full lib.rs:644-651), so the failure
        /// leaves nothing behind.
        pub(in crate::watch) fn tick_rate() -> Option<Duration> {
            if ARMED.with(|armed| armed.get()) {
                Some(crate::watch::QUIET * 2)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use notify_debouncer_full::notify::{
        Event, EventKind,
        event::{
            CreateKind, DataChange, Flag, MetadataKind, ModifyKind,
            RemoveKind, RenameMode,
        },
    };
    use std::time::Instant;

    // ---------------------------------------------------------- note_path

    #[test]
    fn a_typ_file_under_any_category_is_a_note() {
        for dir in ["permanent", "time", "capture", "generated"] {
            let category =
                NoteCategory::from_dir(dir).expect("known category");
            assert_eq!(
                note_path(root(), &root().join(dir).join("a.typ")),
                Some((category, PathBuf::from(dir).join("a.typ"))),
                "{dir} is a category directory"
            );
        }
    }

    #[test]
    fn the_index_directory_is_not_a_note_directory() {
        // the watcher sees SQLite's own writes: if these counted as notes,
        // every rebuild would trigger the next one, forever
        for name in ["index.db", "index.db-journal", "index.db-wal"] {
            assert_eq!(
                note_path(root(), &root().join(".index").join(name)),
                None
            );
        }
    }

    #[test]
    fn templates_non_typ_files_and_nested_paths_are_not_notes() {
        let rejected = [
            "templates/daily.typ",
            "permanent/readme.md",
            "permanent/notes.typ.swp",
            "permanent/sub/deep.typ",
            "permanent",
        ];
        for relative in rejected {
            assert_eq!(
                note_path(root(), &root().join(relative)),
                None,
                "{relative} is not an indexed note"
            );
        }
        assert_eq!(note_path(root(), root()), None);
    }

    #[test]
    fn a_path_outside_the_vault_is_not_a_note() {
        assert_eq!(
            note_path(root(), Path::new("/elsewhere/permanent/a.typ")),
            None
        );
    }

    #[test]
    fn a_directory_whose_name_is_not_utf8_is_not_a_category() {
        // filenames on Linux are bytes, not text: a category directory whose
        // name cannot be read as UTF-8 can never match one we know
        use std::os::unix::ffi::OsStrExt;
        let odd = std::ffi::OsStr::from_bytes(b"perman\xffent");
        assert_eq!(note_path(root(), &root().join(odd).join("a.typ")), None);
    }

    // ------------------------------------------------------------- start

    // `start` is exercised here rather than only from `tests/integration/`
    // because its `new_debouncer` failure is reachable only through the
    // `cfg(test)` fault, and llvm-cov needs one compilation to cover the whole
    // function.

    #[test]
    fn a_started_watcher_reports_a_written_note() {
        let dir = tempfile::tempdir().expect("create tempdir");
        std::fs::create_dir(dir.path().join("permanent"))
            .expect("create category dir");
        let watcher = VaultWatcher::start(dir.path()).expect("start watcher");

        std::fs::write(dir.path().join("permanent/a.typ"), "#meta(id: \"a\")")
            .expect("write note");

        let batch = watcher
            .changes
            .recv_timeout(Duration::from_secs(5))
            .expect("a change batch");
        assert!(batch.contains(&touched("permanent/a.typ")));
    }

    #[test]
    fn a_vault_that_cannot_be_watched_is_reported() {
        let dir = tempfile::tempdir().expect("create tempdir");
        assert!(
            VaultWatcher::start(&dir.path().join("no-such-vault")).is_err()
        );
    }

    #[test]
    fn a_debouncer_that_cannot_be_built_is_reported() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let _armed = faults::arm();
        assert!(VaultWatcher::start(dir.path()).is_err());
    }

    // ---------------------------------------------------------- classify

    #[test]
    fn creating_or_writing_a_note_asks_for_a_reparse() {
        let kinds = [
            EventKind::Create(CreateKind::File),
            EventKind::Create(CreateKind::Any),
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            EventKind::Modify(ModifyKind::Any),
            EventKind::Modify(ModifyKind::Other),
        ];
        for kind in kinds {
            assert_eq!(
                classify_one(kind, &["permanent/a.typ"]),
                vec![touched("permanent/a.typ")],
                "{kind:?} means the note must be reparsed"
            );
        }
    }

    #[test]
    fn removing_a_note_asks_for_a_delete() {
        for kind in [
            EventKind::Remove(RemoveKind::File),
            EventKind::Remove(RemoveKind::Any),
        ] {
            assert_eq!(
                classify_one(kind, &["permanent/a.typ"]),
                vec![removed("permanent/a.typ")],
                "{kind:?} means the note is gone"
            );
        }
    }

    #[test]
    fn reads_and_permission_changes_ask_for_nothing() {
        let kinds = [
            EventKind::Access(notify::event::AccessKind::Read),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Permissions)),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)),
        ];
        for kind in kinds {
            assert_eq!(
                classify_one(kind, &["permanent/a.typ"]),
                Vec::new(),
                "{kind:?} cannot change what a note says"
            );
        }
    }

    #[test]
    fn a_correlated_rename_is_a_delete_then_a_reparse() {
        assert_eq!(
            classify_one(
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &["permanent/old.typ", "permanent/new.typ"],
            ),
            vec![removed("permanent/old.typ"), touched("permanent/new.typ")]
        );
    }

    #[test]
    fn a_rename_with_one_half_outside_the_vault_keeps_the_other_half() {
        let into =
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path(PathBuf::from("/elsewhere/draft.typ"))
                .add_path(root().join("permanent/kept.typ"));
        assert_eq!(classify_event(into), vec![touched("permanent/kept.typ")]);

        let out =
            Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                .add_path(root().join("permanent/gone.typ"))
                .add_path(PathBuf::from("/elsewhere/draft.typ"));
        assert_eq!(classify_event(out), vec![removed("permanent/gone.typ")]);
    }

    #[test]
    fn an_uncorrelated_rename_is_read_from_its_direction() {
        assert_eq!(
            classify_one(
                EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                &["permanent/a.typ"],
            ),
            vec![removed("permanent/a.typ")]
        );
        assert_eq!(
            classify_one(
                EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &["permanent/a.typ"],
            ),
            vec![touched("permanent/a.typ")]
        );
    }

    #[test]
    fn an_undirected_rename_forces_a_rebuild() {
        for mode in [RenameMode::Any, RenameMode::Other] {
            assert_eq!(
                classify_one(
                    EventKind::Modify(ModifyKind::Name(mode)),
                    &["permanent/a.typ"],
                ),
                vec![VaultChange::Rescan],
                "{mode:?} does not say which side of the rename this is"
            );
        }
    }

    #[test]
    fn an_unclassifiable_event_on_a_note_forces_a_rebuild() {
        for kind in [EventKind::Any, EventKind::Other] {
            assert_eq!(
                classify_one(kind, &["permanent/a.typ"]),
                vec![VaultChange::Rescan],
                "{kind:?} says something happened but not what"
            );
        }
    }

    #[test]
    fn an_unclassifiable_event_outside_the_vault_asks_for_nothing() {
        // the ordering that matters: the path filter runs before the kind
        // match, so a write into `.index/` cannot escalate to a rebuild
        for kind in [EventKind::Any, EventKind::Other] {
            assert_eq!(
                classify_one(kind, &[".index/index.db"]),
                Vec::new(),
                "{kind:?} on a non-note is not our business"
            );
        }
    }

    #[test]
    fn a_dropped_event_forces_a_rebuild_whatever_its_path() {
        let flagged = Event::new(EventKind::Create(CreateKind::File))
            .add_path(root().join(".index/index.db"))
            .set_flag(Flag::Rescan);
        assert_eq!(classify_event(flagged), vec![VaultChange::Rescan]);
    }

    #[test]
    fn a_failed_batch_forces_a_rebuild() {
        let failed = Err(vec![notify::Error::generic("backend gave up")]);
        assert_eq!(classify(root(), failed), vec![VaultChange::Rescan]);
    }

    #[test]
    fn an_empty_batch_asks_for_nothing() {
        assert_eq!(classify(root(), Ok(Vec::new())), Vec::new());
    }

    #[test]
    fn every_event_in_a_batch_contributes() {
        let batch = Ok(vec![
            debounced(
                Event::new(EventKind::Create(CreateKind::File))
                    .add_path(root().join("permanent/a.typ")),
            ),
            debounced(
                Event::new(EventKind::Remove(RemoveKind::File))
                    .add_path(root().join("time/b.typ")),
            ),
        ]);
        assert_eq!(
            classify(root(), batch),
            vec![touched("permanent/a.typ"), removed("time/b.typ")]
        );
    }

    // ----------------------------------------------------------- helpers

    fn root() -> &'static Path {
        Path::new("/vault")
    }

    fn classify_one(kind: EventKind, relatives: &[&str]) -> Vec<VaultChange> {
        let event =
            relatives.iter().fold(Event::new(kind), |event, relative| {
                event.add_path(root().join(relative))
            });
        classify_event(event)
    }

    fn classify_event(event: Event) -> Vec<VaultChange> {
        classify(root(), Ok(vec![debounced(event)]))
    }

    fn debounced(event: Event) -> DebouncedEvent {
        DebouncedEvent::new(event, Instant::now())
    }

    fn touched(relative: &str) -> VaultChange {
        let path = PathBuf::from(relative);
        let category = path
            .iter()
            .next()
            .and_then(|dir| dir.to_str())
            .and_then(NoteCategory::from_dir)
            .expect("test paths start with a category directory");
        VaultChange::Touched { category, path }
    }

    fn removed(relative: &str) -> VaultChange {
        VaultChange::Removed(PathBuf::from(relative))
    }
}
