//! The compute tier: every typst compile and every index survey runs
//! through this seam instead of on the UI thread
//! (adr/2026-08-compute-tier-worker-seam.md). Jobs go out through
//! `ComputeFeed::submit`, outcomes come back on one channel a shell task
//! drains into the caches and signals — the watcher bridge's shape,
//! pointed the other way. Two adapters make the seam real: `threaded`
//! (production, two worker lanes) and `inline` (the headless default,
//! which runs a job at submit time so tests stay deterministic).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::domain::{NoteCategory, NoteType};
use crate::index::{Index, IndexError, TableNote};
use crate::loops::{self, LoopLine};
use crate::render::{BodyJob, ExportJob, FragmentJob, RenderTheme};
use crate::watch::{self, VaultChange};
use jiff::civil::Date;

/// What the shell derives from a built index: the rail's time notes, the
/// open loops, the table's notes and the link edges — the survey every
/// batch re-reads whole.
pub type Survey = (
    Vec<(String, NoteType)>,
    Vec<LoopLine>,
    Vec<TableNote>,
    Vec<(String, String)>,
);

/// One unit of off-thread work. Compiles carry their own keys back
/// (`render::FragmentJob`, `render::BodyJob`); a survey carries whether it
/// already is the escalation, so a failed rescan cannot escalate forever.
pub enum Job {
    Fragment(FragmentJob),
    Body(BodyJob),
    Export(ExportJob),
    Survey {
        root: PathBuf,
        batch: Vec<VaultChange>,
        escalated: bool,
        /// The day the due loops are judged against — the shell's one
        /// injected clock, carried in rather than read again here
        /// (adr/2026-07-today-injected-root-context.md).
        today: Date,
    },
}

/// A job's result, landing on the shell's drain: compiles address their
/// cache slot, a survey answers whole or explains itself.
pub enum Outcome {
    Fragment {
        key: u64,
        epoch: u64,
        result: Result<String, String>,
    },
    Body {
        note: PathBuf,
        theme: RenderTheme,
        epoch: u64,
        result: Result<String, String>,
    },
    Survey {
        result: Result<Survey, String>,
        escalated: bool,
    },
    /// The PDF's vault-relative path, or the stage that refused
    /// (adr/2026-09-export-writes-the-pdf-beside-the-note.md).
    Export {
        note: PathBuf,
        result: Result<PathBuf, String>,
    },
}

/// The one executor both adapters share — the seam's whole depth: an
/// adapter decides *where* this runs, never *what* runs.
pub fn run(job: Job) -> Outcome {
    match job {
        Job::Fragment(job) => Outcome::Fragment {
            key: job.key,
            epoch: job.epoch,
            result: job.compile(),
        },
        Job::Body(job) => Outcome::Body {
            note: job.note.clone(),
            theme: job.theme,
            epoch: job.epoch,
            result: job.compile(),
        },
        Job::Survey {
            root,
            batch,
            escalated,
            today,
        } => Outcome::Survey {
            result: refresh(&root, &batch, today),
            escalated,
        },
        Job::Export(job) => Outcome::Export {
            note: job.note.clone(),
            result: job.export(),
        },
    }
}

/// How the shell reaches the tier: `main` injects the threaded adapter,
/// the headless tests inject nothing and get `inline` — the `VaultFeed`
/// pattern, including the receiver taken out of its cell once, by the
/// shell's drain task.
#[derive(Clone)]
pub struct ComputeFeed {
    pub submit: Arc<dyn Fn(Job) + Send + Sync>,
    #[allow(clippy::type_complexity)]
    pub outcomes: Arc<Mutex<Option<UnboundedReceiver<Outcome>>>>,
    /// The inline adapter's mark: probes compile in place and the mount
    /// surveys synchronously, so a first render is complete — exactly the
    /// launch the app had before the tier existed.
    pub inline: bool,
}

/// The headless default: a submitted job runs on the spot and its outcome
/// waits on the channel for the next drain poll — deterministic, ordered,
/// and the shell's async plumbing still runs end to end.
pub fn inline() -> ComputeFeed {
    let (sender, receiver) = unbounded_channel();
    ComputeFeed {
        submit: Arc::new(move |job| {
            let _ = sender.send(run(job));
        }),
        outcomes: Arc::new(Mutex::new(Some(receiver))),
        inline: true,
    }
}

/// Production: two worker lanes, one for compiles and one for surveys, so
/// a zoom's flood of body compiles never delays a watcher batch. Each lane
/// is one thread draining its queue in order — FIFO is what keeps watcher
/// batches applying in arrival order. The threads end when the feed drops
/// their senders; nothing joins them, the outcomes channel just closes.
pub fn threaded() -> ComputeFeed {
    let (sender, receiver) = unbounded_channel();
    let (compiles, _) = lane(sender.clone());
    let (surveys, _) = lane(sender);
    ComputeFeed {
        submit: Arc::new(move |job| {
            let lane = match &job {
                Job::Survey { .. } => &surveys,
                Job::Fragment(_) | Job::Body(_) | Job::Export(_) => &compiles,
            };
            let _ = lane.send(job);
        }),
        outcomes: Arc::new(Mutex::new(Some(receiver))),
        inline: false,
    }
}

/// One worker lane: a thread running jobs in arrival order. The handle is
/// returned so the unit test can join the shutdown path; production drops
/// it and lets the app's exit take the parked thread with it.
fn lane(
    outcomes: UnboundedSender<Outcome>,
) -> (UnboundedSender<Job>, std::thread::JoinHandle<()>) {
    let (sender, mut jobs) = unbounded_channel::<Job>();
    // `loop` with a let-else, not the `while let` clippy would prefer —
    // the house bans while loops outright
    #[allow(clippy::while_let_loop)]
    let handle = std::thread::spawn(move || {
        loop {
            let Some(job) = jobs.blocking_recv() else {
                return;
            };
            if outcomes.send(run(job)).is_err() {
                return;
            }
        }
    });
    (sender, handle)
}

/// One survey computed: the index catches up with the files, then the
/// screen catches up with the index — startup is just this with a
/// `Rescan` batch. Every step reports the same way, so the caller has one
/// message to show rather than three.
pub fn refresh(
    root: &Path,
    batch: &[VaultChange],
    today: Date,
) -> Result<Survey, String> {
    absorb(root, batch, today)
        .map_err(|err| format!("indexing the vault: {err:?}"))
}

/// Open, apply, re-read. The `.index/` directory is ensured here because
/// the first survey of a fresh vault is what creates it — but only inside
/// a vault that exists: a mistyped root must fail the survey, not be
/// silently created and surveyed as empty.
fn absorb(
    root: &Path,
    batch: &[VaultChange],
    today: Date,
) -> Result<Survey, IndexError> {
    if !root.is_dir() {
        return Err(IndexError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no vault at {}", root.display()),
        )));
    }
    let dir = root.join(".index");
    std::fs::create_dir_all(&dir)?;
    let mut index = Index::open(&dir.join("index.db"))?;
    watch::apply(&mut index, root, batch)?;
    survey(&index, today)
}

/// What the shell needs from a built index. Separate from `absorb` so its
/// error arms stay reachable — after a successful rebuild they only fire
/// on a sabotaged database.
pub fn survey(index: &Index, today: Date) -> Result<Survey, IndexError> {
    Ok((
        index.time_notes()?,
        open_loops(index, today)?,
        index.table_notes()?,
        index.link_edges()?,
    ))
}

/// The open loops themselves, not a count of them: the chrome's ember
/// shows this list's length and clicking it shows the list, so the two
/// cannot drift apart (adr/2026-08-loops-list-overlay.md).
pub fn open_loops(
    index: &Index,
    today: Date,
) -> Result<Vec<LoopLine>, IndexError> {
    Ok(loops::lines(
        &index.typeless_notes()?,
        &index.dangling_links()?,
        &index.unsummarized_captures()?,
        &index.anomalies()?,
        &index.due_notes(today)?,
        today,
    ))
}

/// The palette's "export pdf": one note, compiled and written beside
/// itself, off the UI thread like every compile.
pub fn export(root: &Path, note: PathBuf) -> Job {
    Job::Export(ExportJob::new(root, &note))
}

/// A rescan is what a vault entering through this seam always starts
/// with: the batch startup submits, and the batch a failed one escalates
/// to.
pub fn rescan(root: &Path, escalated: bool, today: Date) -> Job {
    Job::Survey {
        root: root.to_path_buf(),
        batch: vec![VaultChange::Rescan],
        escalated,
        today,
    }
}

/// A one-note `Touched` survey: the app's own write seams (create,
/// create a time note, capture) submit this the instant they write a
/// file, instead of waiting on the watcher to notice its own effect
/// (adr/2026-09-the-app-indexes-its-own-writes.md). `path` is
/// vault-relative, the same shape `watch::note_path` yields.
pub fn touched(
    root: &Path,
    category: NoteCategory,
    path: PathBuf,
    today: Date,
) -> Job {
    Job::Survey {
        root: root.to_path_buf(),
        batch: vec![VaultChange::Touched { category, path }],
        escalated: false,
        today,
    }
}

/// A one-note `Removed` survey: the delete seam's counterpart to
/// `touched`, submitted the instant the app removes a file itself.
pub fn removed(root: &Path, path: PathBuf, today: Date) -> Job {
    Job::Survey {
        root: root.to_path_buf(),
        batch: vec![VaultChange::Removed(path)],
        escalated: false,
        today,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::render::DEFAULT_SIZE;

    const TODAY: Date = jiff::civil::date(2026, 7, 24);

    fn fixture_vault() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/vault")
    }

    #[test]
    fn an_inline_feed_answers_at_submit_time() {
        let vault = tempfile::tempdir().expect("tempdir");
        seed_vault(vault.path());
        let feed = inline();
        assert!(feed.inline);
        (feed.submit)(rescan(vault.path(), false, TODAY));
        let outcome = feed
            .outcomes
            .lock()
            .expect("the receiver cell is healthy")
            .as_mut()
            .expect("the receiver is still in its cell")
            .try_recv()
            .expect("the inline adapter already ran the job");
        let Outcome::Survey {
            result: Ok((notes, ..)),
            escalated: false,
        } = outcome
        else {
            panic!("the survey lands whole");
        };
        assert_eq!(notes.len(), 1, "the seeded day is surveyed");
    }

    #[test]
    fn the_threaded_feed_runs_every_job_kind_off_thread() {
        let vault = tempfile::tempdir().expect("tempdir");
        seed_vault(vault.path());
        let feed = threaded();
        assert!(!feed.inline);
        let mut outcomes = feed
            .outcomes
            .lock()
            .expect("the receiver cell is healthy")
            .take()
            .expect("the receiver is still in its cell");

        (feed.submit)(fragment_job(&fixture_vault()));
        (feed.submit)(body_job(&fixture_vault()));
        (feed.submit)(rescan(vault.path(), true, TODAY));
        with_the_shared_template(vault.path());
        (feed.submit)(export(
            vault.path(),
            PathBuf::from("time/2026-07-23.typ"),
        ));

        let mut kinds = (false, false, false, false);
        for _ in 0..4 {
            match outcomes.blocking_recv().expect("a lane answers") {
                Outcome::Fragment { result, .. } => {
                    assert!(
                        result
                            .expect("the fragment compiles")
                            .contains("<svg")
                    );
                    kinds.0 = true;
                }
                Outcome::Body { result, .. } => {
                    assert!(
                        result.expect("the body compiles").contains("<svg")
                    );
                    kinds.1 = true;
                }
                Outcome::Survey { result, escalated } => {
                    assert!(result.is_ok(), "{result:?}");
                    assert!(escalated, "the flag rides through");
                    kinds.2 = true;
                }
                Outcome::Export { note, result } => {
                    assert_eq!(note, PathBuf::from("time/2026-07-23.typ"));
                    assert_eq!(
                        result.expect("the seeded day exports"),
                        PathBuf::from("time/2026-07-23.pdf")
                    );
                    let pdf = std::fs::read(
                        vault.path().join("time/2026-07-23.pdf"),
                    )
                    .expect("the pdf is beside the note");
                    assert!(pdf.starts_with(b"%PDF-"), "a real pdf");
                    kinds.3 = true;
                }
            }
        }
        assert_eq!(kinds, (true, true, true, true), "every lane answered");
    }

    /// The seeded vault compiles once it holds the fixtures' shared
    /// template, which every note imports.
    fn with_the_shared_template(root: &Path) {
        std::fs::copy(
            fixture_vault().join("templates/template.typ"),
            root.join("templates/template.typ"),
        )
        .expect("the shared template is copied");
    }

    #[test]
    fn an_export_names_the_stage_that_refused() {
        let vault = tempfile::tempdir().expect("tempdir");
        seed_vault(vault.path());
        with_the_shared_template(vault.path());
        // a note that is not there: the read refuses
        let Outcome::Export {
            result: Err(gone), ..
        } = run(export(vault.path(), PathBuf::from("time/nope.typ")))
        else {
            panic!("an export answers an export");
        };
        assert!(gone.starts_with("export: "), "{gone}");
        // a note that will not compile: the compile refuses, and no pdf
        std::fs::write(vault.path().join("time/broken.typ"), "#let x = (\n")
            .expect("the broken note is written");
        let Outcome::Export {
            result: Err(broken),
            ..
        } = run(export(vault.path(), PathBuf::from("time/broken.typ")))
        else {
            panic!("an export answers an export");
        };
        assert!(broken.starts_with("export: "), "{broken}");
        assert!(!vault.path().join("time/broken.pdf").exists());
        // a directory the pdf cannot land in: the write refuses
        let time = vault.path().join("time");
        let original =
            std::fs::metadata(&time).expect("time exists").permissions();
        let mut locked = original.clone();
        locked.set_readonly(true);
        std::fs::set_permissions(&time, locked).expect("lock");
        let Outcome::Export {
            result: unwritable, ..
        } = run(export(vault.path(), PathBuf::from("time/2026-07-23.typ")))
        else {
            panic!("an export answers an export");
        };
        std::fs::set_permissions(&time, original).expect("unlock");
        assert!(
            unwritable
                .expect_err("the write refuses")
                .starts_with("export: ")
        );
    }

    #[test]
    fn a_lane_ends_when_its_queue_closes() {
        let (outcomes, _keep) = unbounded_channel();
        let (jobs, worker) = lane(outcomes);
        drop(jobs);
        worker.join().expect("the drained lane returns");
    }

    #[test]
    fn a_lane_ends_when_nobody_listens_for_outcomes() {
        let (outcomes, listener) = unbounded_channel();
        let (jobs, worker) = lane(outcomes);
        drop(listener);
        jobs.send(rescan(Path::new("/nowhere"), false, TODAY))
            .expect("the lane still queues");
        worker.join().expect("the unheard lane returns");
    }

    // -- absorb: the survey's error edges ------------------------------------

    const RESCAN: &[VaultChange] = &[VaultChange::Rescan];

    #[test]
    fn a_missing_vault_fails_at_the_scan() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        let error =
            absorb(&dir.path().join("missing"), RESCAN, TODAY).unwrap_err();
        assert!(matches!(error, IndexError::Io(_)), "{error:?}");
    }

    #[test]
    fn a_file_squatting_the_index_directory_fails_at_creation() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        std::fs::write(dir.path().join(".index"), "not a directory")
            .expect("the squatting file is written");
        let error = absorb(dir.path(), RESCAN, TODAY).unwrap_err();
        assert!(matches!(error, IndexError::Io(_)), "{error:?}");
    }

    #[test]
    fn a_directory_squatting_the_database_fails_at_open() {
        let dir = tempfile::tempdir().expect("a temp dir is available");
        std::fs::create_dir_all(dir.path().join(".index/index.db"))
            .expect("the squatting directory is created");
        let error = absorb(dir.path(), RESCAN, TODAY).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    #[test]
    fn a_read_only_database_fails_at_the_rebuild() {
        let vault = tempfile::tempdir().expect("a temp dir is available");
        seed_vault(vault.path());
        absorb(vault.path(), RESCAN, TODAY).expect("the first build succeeds");
        let db = vault.path().join(".index/index.db");
        let mut permissions = std::fs::metadata(&db)
            .expect("the database exists after the first build")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&db, permissions)
            .expect("the database is made read-only");
        let error = absorb(vault.path(), RESCAN, TODAY).unwrap_err();
        assert!(matches!(error, IndexError::Sqlite(_)), "{error:?}");
    }

    // -- touched / removed: the app's own write seams -----------------------

    #[test]
    fn touched_wraps_a_single_touched_change_unescalated() {
        let root = Path::new("/vault");
        let Job::Survey {
            root: job_root,
            batch,
            escalated,
            today,
        } = touched(
            root,
            NoteCategory::Permanent,
            PathBuf::from("permanent/fresh.typ"),
            TODAY,
        )
        else {
            panic!("touched builds a Survey job");
        };
        assert_eq!(job_root, root);
        assert_eq!(
            batch,
            vec![VaultChange::Touched {
                category: NoteCategory::Permanent,
                path: PathBuf::from("permanent/fresh.typ"),
            }]
        );
        assert!(!escalated, "the app's own write is never an escalation");
        assert_eq!(today, TODAY, "the clock rides along for the due loops");
    }

    #[test]
    fn removed_wraps_a_single_removed_change_unescalated() {
        let root = Path::new("/vault");
        let Job::Survey {
            root: job_root,
            batch,
            escalated,
            ..
        } = removed(root, PathBuf::from("permanent/gone.typ"), TODAY)
        else {
            panic!("removed builds a Survey job");
        };
        assert_eq!(job_root, root);
        assert_eq!(
            batch,
            vec![VaultChange::Removed(PathBuf::from("permanent/gone.typ"))]
        );
        assert!(!escalated, "the app's own delete is never an escalation");
    }

    #[test]
    fn a_touched_job_run_against_a_temp_vault_indexes_the_note() {
        let vault = tempfile::tempdir().expect("tempdir");
        seed_vault(vault.path());
        std::fs::write(
            vault.path().join("permanent/fresh.typ"),
            "#meta(id: \"fresh\", type: \"concept\")\n",
        )
        .expect("write the note the app just created");

        let Outcome::Survey {
            result: Ok((_, _, table, _)),
            escalated: false,
        } = run(touched(
            vault.path(),
            NoteCategory::Permanent,
            PathBuf::from("permanent/fresh.typ"),
            TODAY,
        ))
        else {
            panic!("the survey lands whole");
        };
        assert!(
            table.iter().any(|note| note.id == "fresh"),
            "the note the app just wrote is indexed: {table:?}"
        );
    }

    #[test]
    fn a_removed_job_run_against_a_temp_vault_drops_the_note() {
        let vault = tempfile::tempdir().expect("tempdir");
        seed_vault(vault.path());
        std::fs::write(
            vault.path().join("permanent/fresh.typ"),
            "#meta(id: \"fresh\", type: \"concept\")\n",
        )
        .expect("write the note");
        run(rescan(vault.path(), false, TODAY));
        std::fs::remove_file(vault.path().join("permanent/fresh.typ"))
            .expect("the app's own delete");

        let Outcome::Survey {
            result: Ok((_, _, table, _)),
            escalated: false,
        } = run(removed(
            vault.path(),
            PathBuf::from("permanent/fresh.typ"),
            TODAY,
        ))
        else {
            panic!("the survey lands whole");
        };
        assert!(
            !table.iter().any(|note| note.id == "fresh"),
            "the note the app just deleted is gone: {table:?}"
        );
    }

    #[test]
    fn a_survey_of_nowhere_reports_instead_of_panicking() {
        let Outcome::Survey {
            result: Err(message),
            ..
        } = run(rescan(Path::new("/nowhere"), false, TODAY))
        else {
            panic!("a missing vault is an error, not a survey");
        };
        assert!(message.contains("indexing the vault"), "{message}");
    }

    /// A fragment job the tests can submit: probing a fresh cache is the
    /// only way to mint one, which keeps the test honest about the
    /// interface.
    fn fragment_job(vault: &Path) -> Job {
        let note = vault.join("permanent/zettelkasten.typ");
        let crate::render::FragmentView::Pending { job: Some(job) } =
            crate::render::FragmentCache::default().probe(
                vault,
                &note,
                "= titre\n",
                RenderTheme::Paper(DEFAULT_SIZE),
            )
        else {
            panic!("a fresh cache queues the compile");
        };
        Job::Fragment(job)
    }

    fn body_job(vault: &Path) -> Job {
        let crate::render::BodyView::Pending { job: Some(job), .. } =
            crate::render::BodyCache::default().probe(
                vault,
                Path::new("permanent/zettelkasten.typ"),
                RenderTheme::Paper(DEFAULT_SIZE),
            )
        else {
            panic!("a fresh cache queues the compile");
        };
        Job::Body(job)
    }

    /// A minimal vault the survey can index: the four category directories
    /// and one compilable day note.
    fn seed_vault(root: &Path) {
        for dir in ["permanent", "time", "capture", "generated", "templates"] {
            std::fs::create_dir_all(root.join(dir))
                .expect("the category dirs");
        }
        std::fs::write(
            root.join("time/2026-07-23.typ"),
            "#import \"/templates/template.typ\": *\n#show: note\n\
             #meta(id: \"2026-07-23\", type: \"daily\", created: \"2026-07-23\")\n\
             \n= 2026-07-23\n",
        )
        .expect("the day note is written");
    }
}
