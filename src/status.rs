//! Pure logic behind the status surface: every user-facing message and the
//! liveness fact live here, and nothing else displays a failure
//! (`adr/2026-08-status-surface-owns-notices.md`). Everything decidable
//! without a VirtualDom lives here, so the components stay wiring
//! (`adr/2026-07-ui-covered-at-100.md`).

use crate::editor::Trouble;

/// The one place notices and liveness are held: the shell reports into it,
/// the notice line renders `line()`, the chrome renders `liveness()`, and
/// the palette's notices overlay renders `history()`.
#[derive(Debug)]
pub struct Status {
    /// The notices still standing — at most one per source, and the highest
    /// severity among them wins the line.
    active: Vec<Notice>,
    /// Everything ever reported, oldest first, bounded — the overlay's list,
    /// so an overwritten notice is dismissed from the line, never from the
    /// record.
    history: Vec<Notice>,
    liveness: Liveness,
}

/// One user-facing message: what the notice line shows and the history
/// keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub severity: Severity,
    pub source: Source,
    pub text: String,
}

/// A notice's rank, deciding what may replace it and how it leaves the
/// screen: info yields to any later notice, warning persists until
/// dismissed or resolved, critical persists until acknowledged or resolved
/// (the three-level ladder the ADR chose).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

/// Which subsystem reported: a later report from the same source replaces
/// its predecessor on the line, and a success resolves it — a later clean
/// save clears its own failure without a gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Save,
    /// Its own source, not `Save`: an io failure resolves when a later
    /// save lands, a conflict only when the user picks a side — and the
    /// palette's resolution pair exists exactly while this one stands
    /// (adr/2026-08-external-edit-conflict-commands.md).
    Conflict,
    Positions,
    Watcher,
    Editor,
    Capture,
    Delete,
    Create,
    Index,
    Undo,
    Clipboard,
}

/// Whether the index tracks the vault — rendered by the chrome's liveness
/// glyph in the same place in every state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Watching,
    Unwatched,
    Degraded,
}

/// A runaway failure cannot eat the memory: the oldest entries fall off.
const HISTORY_DEPTH: usize = 50;

impl Default for Status {
    /// Unwatched until the feed proves otherwise: liveness is a fact to
    /// establish, not an assumption to display.
    fn default() -> Status {
        Status {
            active: Vec::new(),
            history: Vec::new(),
            liveness: Liveness::Unwatched,
        }
    }
}

impl Status {
    /// One report: the source's previous notice leaves the line (replaced,
    /// not resolved), any standing info yields, and the history keeps the
    /// entry — deduplicated against its immediate predecessor, so a failure
    /// re-reported every tick records once.
    pub fn report(&mut self, notice: Notice) {
        self.active.retain(|standing| {
            standing.source != notice.source
                && standing.severity != Severity::Info
        });
        if self.history.last() != Some(&notice) {
            self.history.push(notice.clone());
        }
        if self.history.len() > HISTORY_DEPTH {
            self.history.remove(0);
        }
        self.active.push(notice);
    }

    /// What the notice line shows: the highest severity standing, the
    /// latest among equals.
    pub fn line(&self) -> Option<&Notice> {
        self.active.iter().max_by_key(|notice| notice.severity)
    }

    /// Escape at the bottom of the ladder: the visible notice leaves the
    /// line — the explicit gesture a critical requires, and the dismissal a
    /// warning or info accepts. The history keeps it. Returns whether
    /// anything was standing, so the caller's keystroke can stay inert
    /// otherwise.
    pub fn acknowledge(&mut self) -> bool {
        let Some(shown) = self.line().cloned() else {
            return false;
        };
        self.active.retain(|standing| *standing != shown);
        true
    }

    /// The condition a source reported has ceased to hold: its notice
    /// leaves the line without a gesture — resolution beats acknowledgement
    /// (`adr/2026-08-status-surface-owns-notices.md`).
    pub fn resolve(&mut self, source: Source) {
        self.active.retain(|standing| standing.source != source);
    }

    /// Whether a source has a notice standing — the callers' write gate, so
    /// a tick with nothing to change writes nothing.
    pub fn has(&self, source: Source) -> bool {
        self.active.iter().any(|standing| standing.source == source)
    }

    /// Whether this exact notice already stands — the re-report gate, so a
    /// failure repeating every tick repaints nothing.
    pub fn showing(&self, notice: &Notice) -> bool {
        self.active.iter().any(|standing| standing == notice)
    }

    pub fn liveness(&self) -> Liveness {
        self.liveness
    }

    pub fn set_liveness(&mut self, liveness: Liveness) {
        self.liveness = liveness;
    }

    /// Every notice ever reported, oldest first — the overlay reverses it.
    pub fn history(&self) -> &[Notice] {
        &self.history
    }
}

/// Every message the app can show, built here and nowhere else — the prose
/// is reviewed in one place (`adr/2026-08-status-surface-owns-notices.md`).
impl Notice {
    /// The editor's own trouble, forwarded by the shell: a diverged widget
    /// edit, a note that would not open, a flush that failed — the last one
    /// critical, because typed text is not on disk.
    pub fn from_trouble(trouble: Trouble) -> Notice {
        match trouble {
            Trouble::Stale => Notice {
                severity: Severity::Warning,
                source: Source::Editor,
                text: "edit dropped: the editor lost its block".to_string(),
            },
            Trouble::Open(detail) => Notice {
                severity: Severity::Warning,
                source: Source::Editor,
                text: detail,
            },
            Trouble::Save(detail) => Notice::save_failed(&detail),
            Trouble::Conflict => Notice::conflicted(),
        }
    }

    /// The external-edit guard refusing to clobber: both versions survive
    /// — the buffer in memory, the other author's on disk — until the user
    /// picks a side (adr/2026-08-external-edit-conflict-commands.md).
    pub fn conflicted() -> Notice {
        Notice {
            severity: Severity::Critical,
            source: Source::Conflict,
            text: "save: the note changed on disk — \
                   keep mine or take disk decides"
                .to_string(),
        }
    }

    /// The autosave's and the flush's failure: the data-loss class, so the
    /// ladder's top — resolved by the next save that lands.
    /// A `:` line the grammar could not read, or one that found nothing to
    /// do. `detail` already carries what happened and what to type instead
    /// (AIR ERR-1, adr/2026-08-ex-line-is-literal-and-global.md); a typo in
    /// a prompt destroys no work, so it warns rather than shouting.
    pub fn ex_refused(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Editor,
            text: detail.to_string(),
        }
    }

    pub fn save_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Critical,
            source: Source::Save,
            text: format!(
                "save: {detail} — the text survives in the buffer; \
                 the next pause retries"
            ),
        }
    }

    /// The positions file refusing the debounced write: user data
    /// (adr/2026-07-positions-separate-file.md), the same class as a note.
    pub fn positions_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Critical,
            source: Source::Positions,
            text: format!(
                "positions: {detail} — placements held in memory; \
                 the next move retries"
            ),
        }
    }

    /// A watcher batch the index refused: the screen may be behind the
    /// vault, and the queued rescan is how it heals
    /// (`adr/2026-08-failed-batch-escalates-to-rescan.md`).
    pub fn watcher_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Watcher,
            text: format!("{detail} — the index may be behind; rescanning"),
        }
    }

    /// A watcher that never started: the launch index is all there is.
    pub fn unwatched(reason: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Watcher,
            text: format!(
                "the vault is not watched: {reason} — outside edits \
                 will not appear"
            ),
        }
    }

    /// A vault whose skeleton would not seed: the default templates the
    /// binary carries could not land, so creating notes may fail too
    /// (adr/2026-08-templates-seeded-from-embedded-fixtures.md).
    pub fn seed_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Create,
            text: format!("templates: {detail} — creating notes may fail"),
        }
    }

    /// A capture that landed: the id is the receipt
    /// (adr/2026-08-capture-timestamp-ids.md).
    pub fn captured(stem: &str) -> Notice {
        Notice {
            severity: Severity::Info,
            source: Source::Capture,
            text: format!("captured {stem}"),
        }
    }

    pub fn capture_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Capture,
            text: format!("capture: {detail}"),
        }
    }

    /// A native clipboard read that did not yield text. The note is
    /// untouched, and another copy/paste attempt is the recovery path.
    pub fn clipboard_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Clipboard,
            text: format!(
                "clipboard: {detail} — no text was read; copy it again and retry"
            ),
        }
    }

    /// A delete the filesystem refused: nothing is half-deleted
    /// (adr/2026-08-delete-note-palette-only-from-sheet.md).
    pub fn delete_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Delete,
            text: format!("delete: {detail} — the note is still on disk"),
        }
    }

    /// An undo that could not restore its note — most often because the
    /// path holds a living file again; the register never overwrites one
    /// (adr/2026-08-app-level-undo-register.md).
    pub fn undo_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Undo,
            text: format!("undo: {detail}"),
        }
    }

    /// A time note the template would not create; the permanent-note
    /// creator keeps its own overlay-local message, where the overlay
    /// occludes this line (adr/2026-08-ctrl-n-two-step-create-overlay.md).
    pub fn create_failed(detail: &str) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Create,
            text: format!("create: {detail}"),
        }
    }

    /// An index read that failed on the way to an overlay or a sheet — the
    /// caller's message arrives already naming its context ("links: …",
    /// "sheet: …"), the one family whose prose predates this module.
    pub fn index(detail: String) -> Notice {
        Notice {
            severity: Severity::Warning,
            source: Source::Index,
            text: detail,
        }
    }

    /// The notice line's class, one per severity — the non-colour channel
    /// is the weight, and the palette maps the classes in `theme.css`.
    pub fn class(&self) -> &'static str {
        match self.severity {
            Severity::Info => "notice-info",
            Severity::Warning => "notice-warning",
            Severity::Critical => "notice-critical",
        }
    }
}

impl Liveness {
    /// The glyph's class — dim when watching, the alert hue otherwise, with
    /// the fill as the non-colour channel (theme.css § chrome).
    pub fn class(self) -> &'static str {
        match self {
            Liveness::Watching => "liveness-watching",
            Liveness::Unwatched => "liveness-unwatched",
            Liveness::Degraded => "liveness-degraded",
        }
    }

    /// The glyph is a ring while watching and fills when the index may be
    /// behind — the state survives greyscale.
    pub fn filled(self) -> bool {
        self != Liveness::Watching
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn info(text: &str) -> Notice {
        Notice {
            severity: Severity::Info,
            source: Source::Capture,
            text: text.to_string(),
        }
    }

    #[test]
    fn the_line_starts_empty_and_the_vault_starts_unwatched() {
        let status = Status::default();
        assert_eq!(status.line(), None);
        assert_eq!(status.liveness(), Liveness::Unwatched);
        assert!(status.history().is_empty());
    }

    #[test]
    fn the_highest_severity_wins_the_line_and_the_latest_breaks_ties() {
        let mut status = Status::default();
        status.report(Notice::captured("c-1"));
        status.report(Notice::save_failed("disk full"));
        status.report(Notice::delete_failed("read-only"));
        let line = status.line().expect("something is standing");
        assert_eq!(line.severity, Severity::Critical, "{line:?}");

        let mut ties = Status::default();
        ties.report(Notice::delete_failed("first"));
        ties.report(Notice::capture_failed("second"));
        let line = ties.line().expect("something is standing");
        assert!(line.text.contains("second"), "{line:?}");
    }

    #[test]
    fn a_source_replaces_its_own_notice_instead_of_stacking() {
        let mut status = Status::default();
        status.report(Notice::save_failed("first"));
        status.report(Notice::save_failed("second"));
        status.acknowledge();
        assert_eq!(status.line(), None, "one standing notice, not two");
        assert_eq!(status.history().len(), 2, "the record keeps both");
    }

    #[test]
    fn info_yields_to_any_later_notice() {
        let mut status = Status::default();
        status.report(info("captured c-1"));
        status.report(Notice::delete_failed("read-only"));
        status.acknowledge();
        assert_eq!(status.line(), None, "the info yielded, not queued");
    }

    #[test]
    fn acknowledge_clears_the_visible_notice_and_only_it() {
        let mut status = Status::default();
        status.report(Notice::watcher_failed("watching the vault: boom"));
        status.report(Notice::save_failed("disk full"));
        assert!(status.acknowledge(), "the critical was showing");
        let line = status.line().expect("the warning now shows");
        assert_eq!(line.severity, Severity::Warning);
        assert!(status.acknowledge());
        assert!(!status.acknowledge(), "nothing left to acknowledge");
    }

    #[test]
    fn resolution_clears_a_source_without_a_gesture() {
        let mut status = Status::default();
        status.report(Notice::save_failed("disk full"));
        assert!(status.has(Source::Save));
        status.resolve(Source::Save);
        assert!(!status.has(Source::Save));
        assert_eq!(status.line(), None);
        assert_eq!(status.history().len(), 1, "the record survives");
    }

    #[test]
    fn showing_gates_a_re_report_and_history_dedups_it() {
        let mut status = Status::default();
        let notice = Notice::save_failed("disk full");
        assert!(!status.showing(&notice));
        status.report(notice.clone());
        assert!(status.showing(&notice));
        status.report(notice.clone());
        assert_eq!(status.history().len(), 1, "the same failure, one entry");
    }

    #[test]
    fn the_history_is_bounded() {
        let mut status = Status::default();
        for count in 0..(HISTORY_DEPTH + 5) {
            status.report(info(&format!("captured c-{count}")));
        }
        assert_eq!(status.history().len(), HISTORY_DEPTH);
        let first = status.history().first().expect("the record is full");
        assert!(
            first.text.ends_with("c-5"),
            "the oldest fell off: {first:?}"
        );
    }

    #[test]
    fn liveness_is_a_settable_fact() {
        let mut status = Status::default();
        status.set_liveness(Liveness::Watching);
        assert_eq!(status.liveness(), Liveness::Watching);
        status.set_liveness(Liveness::Degraded);
        assert_eq!(status.liveness(), Liveness::Degraded);
    }

    #[test]
    fn every_trouble_maps_to_its_severity() {
        let stale = Notice::from_trouble(Trouble::Stale);
        assert_eq!(stale.severity, Severity::Warning);
        assert!(stale.text.contains("edit dropped"), "{stale:?}");

        let open =
            Notice::from_trouble(Trouble::Open("a.typ: missing".to_string()));
        assert_eq!(open.severity, Severity::Warning);
        assert_eq!(open.text, "a.typ: missing");

        let save =
            Notice::from_trouble(Trouble::Save("a.typ: full".to_string()));
        assert_eq!(save.severity, Severity::Critical);
        assert_eq!(save.source, Source::Save);
        assert!(save.text.contains("a.typ: full"), "{save:?}");

        let conflict = Notice::from_trouble(Trouble::Conflict);
        assert_eq!(conflict.severity, Severity::Critical);
        assert_eq!(conflict.source, Source::Conflict);
        assert!(conflict.text.contains("changed on disk"), "{conflict:?}");
        assert!(conflict.text.contains("keep mine"), "{conflict:?}");
    }

    #[test]
    fn every_message_names_its_situation() {
        assert!(
            Notice::watcher_failed("watching the vault: boom")
                .text
                .contains("rescanning")
        );
        assert!(
            Notice::unwatched("inotify refused")
                .text
                .contains("outside edits")
        );
        assert_eq!(Notice::captured("c-1").text, "captured c-1");
        assert!(Notice::capture_failed("boom").text.starts_with("capture:"));
        assert!(
            Notice::clipboard_failed("denied")
                .text
                .starts_with("clipboard: denied")
        );
        assert!(Notice::delete_failed("boom").text.contains("still on disk"));
        assert!(Notice::create_failed("boom").text.starts_with("create:"));
        assert_eq!(
            Notice::index("links: boom".to_string()).text,
            "links: boom"
        );
    }

    #[test]
    fn the_classes_map_severity_and_liveness_for_the_stylesheet() {
        assert_eq!(Notice::captured("c").class(), "notice-info");
        assert_eq!(Notice::delete_failed("x").class(), "notice-warning");
        assert_eq!(Notice::save_failed("x").class(), "notice-critical");
        assert_eq!(Liveness::Watching.class(), "liveness-watching");
        assert_eq!(Liveness::Unwatched.class(), "liveness-unwatched");
        assert_eq!(Liveness::Degraded.class(), "liveness-degraded");
        assert!(!Liveness::Watching.filled());
        assert!(Liveness::Unwatched.filled());
        assert!(Liveness::Degraded.filled());
    }
}
