//! Pure logic behind the Ctrl+P command palette: the registry of every
//! user-invocable command and what a query narrows it to. Everything
//! decidable without a VirtualDom lives here, so the component stays wiring
//! (`adr/2026-07-ui-covered-at-100.md`). The registry is the app's complete
//! named surface — every phase that adds a keystroke adds its entry in the
//! same change (`adr/2026-08-palette-birth-command-list.md`).

/// Every command the palette can run. The dispatch in `ui` matches this
/// exhaustively with no wildcard arm, so a variant added here does not
/// compile until it is wired.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommandId {
    ToggleTheme,
    Quit,
    CaptureClipboard,
    InsertLink,
    FollowLink,
    PreviousMonth,
    NextMonth,
    OpenLoops,
    GoToToday,
    GoToTable,
    GoToLogs,
    NewNote,
    DeleteNote,
    ZoomToBodies,
    ZoomToTitles,
    FilterCards,
    JumpToNote,
    ArrangeCluster,
}

/// One palette row: the plain English name a command is found by, and the
/// chord it also answers to — `None` for the mouse-only commands.
#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub id: CommandId,
    pub label: &'static str,
    pub chord: Option<&'static str>,
}

/// The birth command list (`adr/2026-08-palette-birth-command-list.md`)
/// plus the v1 phase-2 screen commands
/// (`adr/2026-08-screen-switch-gesture.md`), in the order the palette shows
/// it.
pub const COMMANDS: [Command; 18] = [
    Command {
        id: CommandId::ToggleTheme,
        label: "toggle theme",
        chord: Some("ctrl+t"),
    },
    Command {
        id: CommandId::Quit,
        label: "quit",
        chord: Some("ctrl+q"),
    },
    Command {
        id: CommandId::CaptureClipboard,
        label: "capture clipboard",
        chord: Some("ctrl+shift+v"),
    },
    Command {
        id: CommandId::InsertLink,
        label: "insert link",
        chord: Some("ctrl+l"),
    },
    Command {
        id: CommandId::FollowLink,
        label: "follow link",
        chord: Some("ctrl+enter"),
    },
    Command {
        id: CommandId::PreviousMonth,
        label: "previous month",
        chord: Some("←"),
    },
    Command {
        id: CommandId::NextMonth,
        label: "next month",
        chord: Some("→"),
    },
    Command {
        id: CommandId::OpenLoops,
        label: "open loops",
        chord: None,
    },
    Command {
        id: CommandId::GoToToday,
        label: "go to today",
        chord: None,
    },
    Command {
        id: CommandId::GoToTable,
        label: "go to table",
        chord: Some("ctrl+1"),
    },
    Command {
        id: CommandId::GoToLogs,
        label: "go to logs",
        chord: Some("ctrl+2"),
    },
    Command {
        id: CommandId::NewNote,
        label: "new note",
        chord: Some("ctrl+n"),
    },
    // deliberately chordless: destruction earns a summon-and-name, never a
    // keystroke (adr/2026-08-delete-note-palette-only-from-sheet.md)
    Command {
        id: CommandId::DeleteNote,
        label: "delete note",
        chord: None,
    },
    Command {
        id: CommandId::ZoomToBodies,
        label: "zoom to bodies",
        chord: Some("ctrl+="),
    },
    Command {
        id: CommandId::ZoomToTitles,
        label: "zoom to titles",
        chord: Some("ctrl+-"),
    },
    Command {
        id: CommandId::FilterCards,
        label: "filter cards",
        chord: Some("ctrl+f"),
    },
    Command {
        id: CommandId::JumpToNote,
        label: "jump to note",
        chord: Some("ctrl+o"),
    },
    // chordless: layout is rare and deliberate — "at most a command"
    // (adr/2026-08-arrange-cluster-command.md)
    Command {
        id: CommandId::ArrangeCluster,
        label: "arrange cluster",
        chord: None,
    },
];

/// What was true when the palette opened — decides which commands exist at
/// all. Two flags: the caret commands need a block to act on (the caret
/// itself is probed at run time, never to decide visibility), and the
/// screen commands hide where they already stand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Context {
    pub block_active: bool,
    pub on_table: bool,
    /// Whether a sheet is open: delete acts on the note the sheet shows,
    /// so without one there is nothing to name
    /// (adr/2026-08-delete-note-palette-only-from-sheet.md).
    pub sheet_open: bool,
    /// Whether the table stands at body zoom: each zoom command hides at
    /// its own level — going where you stand is not a command
    /// (adr/2026-08-body-zoom-scale-and-metrics.md).
    pub at_bodies: bool,
}

/// The rows a query leaves: the available commands whose label contains the
/// query, case-insensitively — the picker's matching rule (`links::filter`).
/// An empty query is the whole vocabulary, uncapped: unlike the vault, the
/// registry is bounded, and seeing all of it is the point
/// (`adr/2026-08-command-palette-overlay-shape.md`).
pub fn filter(query: &str, context: Context) -> Vec<&'static Command> {
    let needle = query.to_lowercase();
    COMMANDS
        .iter()
        .filter(|command| available(command.id, context))
        .filter(|command| command.label.to_lowercase().contains(&needle))
        .collect()
}

/// Whether a command exists in this context: hidden beats disabled — a
/// visible dead command teaches a false vocabulary
/// (`adr/2026-08-palette-birth-command-list.md`).
fn available(id: CommandId, context: Context) -> bool {
    match id {
        CommandId::InsertLink | CommandId::FollowLink => context.block_active,
        // going where you stand is not a command
        CommandId::GoToTable => !context.on_table,
        CommandId::GoToLogs => context.on_table,
        CommandId::DeleteNote => context.sheet_open,
        CommandId::ZoomToBodies => context.on_table && !context.at_bodies,
        CommandId::ZoomToTitles => context.on_table && context.at_bodies,
        // the finders act on cards, which only the table shows
        CommandId::FilterCards | CommandId::JumpToNote => context.on_table,
        // the arrange scopes to the open sheet's component
        CommandId::ArrangeCluster => context.sheet_open,
        _ => true,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const EDITING: Context = Context {
        block_active: true,
        on_table: false,
        sheet_open: false,
        at_bodies: false,
    };
    const READING: Context = Context {
        block_active: false,
        on_table: false,
        sheet_open: false,
        at_bodies: false,
    };
    const AT_TABLE: Context = Context {
        block_active: false,
        on_table: true,
        sheet_open: false,
        at_bodies: false,
    };
    const AT_SHEET: Context = Context {
        block_active: false,
        on_table: true,
        sheet_open: true,
        at_bodies: false,
    };
    const AT_BODIES: Context = Context {
        block_active: false,
        on_table: true,
        sheet_open: false,
        at_bodies: true,
    };

    fn labels(rows: &[&Command]) -> Vec<&'static str> {
        rows.iter().map(|command| command.label).collect()
    }

    #[test]
    fn the_query_narrows_by_label_ignoring_case() {
        assert_eq!(labels(&filter("theme", EDITING)), vec!["toggle theme"]);
        assert_eq!(
            labels(&filter("MONTH", EDITING)),
            vec!["previous month", "next month"]
        );
        assert_eq!(filter("xyzzy", EDITING), Vec::<&Command>::new());
    }

    #[test]
    fn an_empty_query_is_the_whole_registry_in_order() {
        // the whole vocabulary minus the place already stood in, the
        // sheet-bound command no sheet backs, and the table-bound zooms
        assert_eq!(
            labels(&filter("", EDITING)),
            COMMANDS
                .iter()
                .map(|c| c.label)
                .filter(|label| {
                    *label != "go to logs"
                        && *label != "delete note"
                        && *label != "arrange cluster"
                        && !label.starts_with("zoom")
                        && *label != "filter cards"
                        && *label != "jump to note"
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_active_block_hides_the_caret_commands() {
        let visible = labels(&filter("", READING));
        assert_eq!(visible.len(), COMMANDS.len() - 9);
        assert!(!visible.contains(&"insert link"));
        assert!(!visible.contains(&"follow link"));
    }

    #[test]
    fn the_finders_hide_off_the_table() {
        assert_eq!(labels(&filter("filter", READING)), Vec::<&str>::new());
        assert_eq!(labels(&filter("jump", READING)), Vec::<&str>::new());
        assert_eq!(labels(&filter("filter", AT_TABLE)), vec!["filter cards"]);
        assert_eq!(labels(&filter("jump", AT_TABLE)), vec!["jump to note"]);
    }

    #[test]
    fn the_zoom_commands_hide_off_the_table_and_at_their_own_level() {
        assert_eq!(labels(&filter("zoom", READING)), Vec::<&str>::new());
        assert_eq!(labels(&filter("zoom", AT_TABLE)), vec!["zoom to bodies"]);
        assert_eq!(labels(&filter("zoom", AT_BODIES)), vec!["zoom to titles"]);
    }

    #[test]
    fn delete_note_exists_only_over_an_open_sheet() {
        assert!(!labels(&filter("delete", AT_TABLE)).contains(&"delete note"));
        assert_eq!(labels(&filter("delete", AT_SHEET)), vec!["delete note"]);
    }

    #[test]
    fn arrange_cluster_exists_only_over_an_open_sheet() {
        assert_eq!(labels(&filter("arrange", AT_TABLE)), Vec::<&str>::new());
        assert_eq!(
            labels(&filter("arrange", AT_SHEET)),
            vec!["arrange cluster"]
        );
    }

    #[test]
    fn the_screen_commands_hide_where_they_stand() {
        let on_logs = labels(&filter("", READING));
        assert!(on_logs.contains(&"go to table"));
        assert!(!on_logs.contains(&"go to logs"));
        let on_table = labels(&filter("", AT_TABLE));
        assert!(on_table.contains(&"go to logs"));
        assert!(!on_table.contains(&"go to table"));
    }

    /// The completeness audit the roadmap demands: the registry against the
    /// chords the app answers. A new chord must touch this list — and its
    /// palette entry — in the same change.
    #[test]
    fn the_registered_set_matches_the_apps_chords() {
        let chords: Vec<&str> = COMMANDS
            .iter()
            .filter_map(|command| command.chord)
            .collect();
        assert_eq!(
            chords,
            vec![
                "ctrl+t",
                "ctrl+q",
                "ctrl+shift+v",
                "ctrl+l",
                "ctrl+enter",
                "←",
                "→",
                "ctrl+1",
                "ctrl+2",
                "ctrl+n",
                "ctrl+=",
                "ctrl+-",
                "ctrl+f",
                "ctrl+o",
            ]
        );
        let chordless: Vec<&str> = COMMANDS
            .iter()
            .filter(|command| command.chord.is_none())
            .map(|command| command.label)
            .collect();
        assert_eq!(
            chordless,
            vec![
                "open loops",
                "go to today",
                "delete note",
                "arrange cluster"
            ]
        );
        let mut names: Vec<&str> =
            COMMANDS.iter().map(|command| command.label).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len(), "labels must be unique");
    }
}
