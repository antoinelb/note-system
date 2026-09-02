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
    Back,
    SearchText,
    InsertLink,
    FollowLink,
    OpenLoops,
    OpenDaily,
    PreviousDaily,
    NextDaily,
    OpenWeekly,
    PreviousWeekly,
    NextWeekly,
    OpenSeason,
    PreviousSeason,
    NextSeason,
    GoToTable,
    GoToLogs,
    NewNote,
    DeleteNote,
    Notices,
    KeepMine,
    TakeDisk,
    ZoomToBodies,
    ZoomToTitles,
    FilterCards,
    FoldRail,
    FoldJump,
    JumpToNote,
    ArrangeCluster,
    Undo,
    EditTemplate,
    ExportPdf,
    OpenSettings,
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
/// (`adr/2026-08-screen-switch-gesture.md`), alphabetized by label
/// (`adr/2026-08-palette-order-and-overlay-placement.md`) — the order the
/// palette shows it in.
pub const COMMANDS: [Command; 34] = [
    // chordless: layout is rare and deliberate — "at most a command"
    // (adr/2026-08-arrange-cluster-command.md)
    Command {
        id: CommandId::ArrangeCluster,
        label: "arrange cluster",
        chord: None,
    },
    // unconfirmed, undoable: the sheet-open guard and the shift modifier
    // are what make a delete chord acceptable
    // (adr/2026-08-delete-note-chord.md)
    Command {
        id: CommandId::DeleteNote,
        label: "delete note",
        chord: None,
    },
    // chordless: reshaping what every future note looks like is rare and
    // deliberate (adr/2026-08-template-editing-in-the-one-editor.md)
    Command {
        id: CommandId::EditTemplate,
        label: "edit template",
        chord: None,
    },
    // chordless: a pdf is asked for, not typed into
    // (adr/2026-09-export-writes-the-pdf-beside-the-note.md)
    Command {
        id: CommandId::ExportPdf,
        label: "export pdf",
        chord: None,
    },
    Command {
        id: CommandId::FilterCards,
        label: "filter cards",
        chord: Some("ctrl+f"),
    },
    // the temporal panes fold on the logs alone
    // (adr/2026-09-alt-h-and-alt-l-fold-the-temporal-panes.md)
    Command {
        id: CommandId::FoldJump,
        label: "fold jump panel",
        chord: Some("alt+l"),
    },
    Command {
        id: CommandId::FoldRail,
        label: "fold rail",
        chord: Some("alt+h"),
    },
    Command {
        id: CommandId::FollowLink,
        label: "follow link",
        chord: Some("ctrl+enter"),
    },
    Command {
        id: CommandId::GoToLogs,
        label: "go to logs",
        chord: Some("ctrl+2"),
    },
    Command {
        id: CommandId::GoToTable,
        label: "go to table",
        chord: Some("ctrl+1"),
    },
    Command {
        id: CommandId::InsertLink,
        label: "insert link",
        chord: Some("ctrl+l"),
    },
    Command {
        id: CommandId::JumpToNote,
        label: "jump to note",
        chord: Some("ctrl+o"),
    },
    // the conflict's fork, chordless like delete: picking a side between
    // two authors earns a summon-and-name
    // (adr/2026-08-external-edit-conflict-commands.md)
    Command {
        id: CommandId::KeepMine,
        label: "keep mine",
        chord: None,
    },
    Command {
        id: CommandId::NewNote,
        label: "new note",
        chord: Some("ctrl+n"),
    },
    // the status history: everything the notice line ever showed
    // (adr/2026-08-status-surface-owns-notices.md)
    Command {
        id: CommandId::Notices,
        label: "notices",
        chord: None,
    },
    Command {
        id: CommandId::OpenDaily,
        label: "open daily",
        chord: Some("ctrl+d"),
    },
    Command {
        id: CommandId::OpenLoops,
        label: "open loops",
        chord: None,
    },
    Command {
        id: CommandId::NextDaily,
        label: "open next daily",
        chord: None,
    },
    Command {
        id: CommandId::NextSeason,
        label: "open next season",
        chord: None,
    },
    Command {
        id: CommandId::NextWeekly,
        label: "open next weekly",
        chord: None,
    },
    Command {
        id: CommandId::PreviousDaily,
        label: "open previous daily",
        chord: None,
    },
    Command {
        id: CommandId::PreviousSeason,
        label: "open previous season",
        chord: None,
    },
    Command {
        id: CommandId::PreviousWeekly,
        label: "open previous weekly",
        chord: None,
    },
    Command {
        id: CommandId::OpenSeason,
        label: "open season",
        chord: None,
    },
    Command {
        id: CommandId::OpenWeekly,
        label: "open weekly",
        chord: None,
    },
    Command {
        id: CommandId::Quit,
        label: "quit",
        chord: Some("ctrl+q"),
    },
    // the visit log's picker, Obsidian's Ctrl+O-style switcher
    // (adr/2026-08-ctrl-b-recent-notes-picker.md)
    Command {
        id: CommandId::Back,
        label: "recent notes",
        chord: Some("ctrl+b"),
    },
    // the vault's text, not the table's cards: Ctrl+F stays the filter
    // (adr/2026-09-full-text-search-lives-in-the-index.md)
    Command {
        id: CommandId::SearchText,
        label: "search text",
        chord: Some("ctrl+shift+f"),
    },
    // the theme toggle and font-size stepper, session-only
    // (adr/2026-08-settings-overlay.md)
    Command {
        id: CommandId::OpenSettings,
        label: "settings",
        chord: Some("ctrl+,"),
    },
    Command {
        id: CommandId::TakeDisk,
        label: "take disk",
        chord: None,
    },
    // ctrl+t went to the todo toggle (adr/2026-08-ctrl-t-toggles-the-todo.md);
    // toggling the theme is now found only by name
    Command {
        id: CommandId::ToggleTheme,
        label: "toggle theme",
        chord: None,
    },
    // found by "undo"; the rendered row wears the register's own words
    // for what it would take back — "undo delete <id>", "undo arrange"
    // (adr/2026-08-app-level-undo-register.md). Chordless like delete:
    // the reverse of a summon-and-name is a summon-and-name.
    Command {
        id: CommandId::Undo,
        label: "undo",
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
];

/// What was true when the palette opened — decides which commands exist at
/// all. Two flags: the caret commands need a block to act on (the caret
/// itself is probed at run time, never to decide visibility), and the
/// screen commands hide where they already stand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Context {
    pub block_active: bool,
    /// Whether the editor holds a note at all: the export writes the
    /// note's own pdf, and a closed editor names none
    /// (adr/2026-09-export-writes-the-pdf-beside-the-note.md).
    pub note_open: bool,
    pub on_table: bool,
    /// Whether a sheet is open: delete acts on the note the sheet shows,
    /// so without one there is nothing to name
    /// (adr/2026-08-delete-note-palette-only-from-sheet.md).
    pub sheet_open: bool,
    /// Whether the table stands at body zoom: each zoom command hides at
    /// its own level — going where you stand is not a command
    /// (adr/2026-08-body-zoom-scale-and-metrics.md).
    pub at_bodies: bool,
    /// Whether a save stands refused over an external edit: the resolution
    /// pair exists only while there is a side to pick
    /// (adr/2026-08-external-edit-conflict-commands.md).
    pub conflict: bool,
    /// Whether the undo register holds anything: with nothing to take
    /// back, "undo" would be a visible dead command
    /// (adr/2026-08-app-level-undo-register.md).
    pub undoable: bool,
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
        // the caret commands need their block *visible*: on the table the
        // editor's block lives behind the screen unless a sheet shows it
        // (adr/2026-08-cursor-always-in-the-note.md)
        CommandId::InsertLink | CommandId::FollowLink => {
            context.block_active && (!context.on_table || context.sheet_open)
        }
        // the note has to be on screen to be the one exported: the logs
        // show it, the bare table hides it behind the screen, a sheet
        // shows it again
        CommandId::ExportPdf => {
            context.note_open && (!context.on_table || context.sheet_open)
        }
        // going where you stand is not a command
        CommandId::GoToTable => !context.on_table,
        CommandId::GoToLogs => context.on_table,
        CommandId::DeleteNote => context.sheet_open,
        CommandId::ZoomToBodies => context.on_table && !context.at_bodies,
        CommandId::ZoomToTitles => context.on_table && context.at_bodies,
        // the finders act on cards, which only the table shows
        CommandId::FilterCards | CommandId::JumpToNote => context.on_table,
        // the panes are the logs'
        CommandId::FoldRail | CommandId::FoldJump => !context.on_table,
        // the arrange scopes to the open sheet's component
        CommandId::ArrangeCluster => context.sheet_open,
        // no conflict, no sides to pick
        CommandId::KeepMine | CommandId::TakeDisk => context.conflict,
        // nothing held, nothing to take back
        CommandId::Undo => context.undoable,
        // the template opens in the logs' centre pane — the one full-page
        // surface the shared editor has off the table
        // (adr/2026-08-template-editing-in-the-one-editor.md)
        CommandId::EditTemplate => !context.on_table,
        _ => true,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const EDITING: Context = Context {
        block_active: true,
        note_open: true,
        on_table: false,
        sheet_open: false,
        at_bodies: false,
        conflict: false,
        undoable: false,
    };
    const READING: Context = Context {
        block_active: false,
        note_open: true,
        on_table: false,
        sheet_open: false,
        at_bodies: false,
        conflict: false,
        undoable: false,
    };
    const AT_TABLE: Context = Context {
        block_active: false,
        note_open: true,
        on_table: true,
        sheet_open: false,
        at_bodies: false,
        conflict: false,
        undoable: false,
    };
    const AT_SHEET: Context = Context {
        block_active: false,
        note_open: true,
        on_table: true,
        sheet_open: true,
        at_bodies: false,
        conflict: false,
        undoable: false,
    };
    const AT_BODIES: Context = Context {
        block_active: false,
        note_open: true,
        on_table: true,
        sheet_open: false,
        at_bodies: true,
        conflict: false,
        undoable: false,
    };
    const CONFLICTED: Context = Context {
        conflict: true,
        ..READING
    };

    fn labels(rows: &[&Command]) -> Vec<&'static str> {
        rows.iter().map(|command| command.label).collect()
    }

    #[test]
    fn the_query_narrows_by_label_ignoring_case() {
        assert_eq!(labels(&filter("theme", EDITING)), vec!["toggle theme"]);
        assert_eq!(
            labels(&filter("WEEKLY", EDITING)),
            vec!["open next weekly", "open previous weekly", "open weekly"]
        );
        assert_eq!(filter("xyzzy", EDITING), Vec::<&Command>::new());
    }

    #[test]
    fn the_time_navigation_commands_are_found_by_label() {
        assert_eq!(
            labels(&filter("previous", READING)),
            vec![
                "open previous daily",
                "open previous season",
                "open previous weekly"
            ]
        );
        assert_eq!(
            labels(&filter("next", READING)),
            vec!["open next daily", "open next season", "open next weekly"]
        );
        assert_eq!(
            labels(&filter("daily", READING)),
            vec!["open daily", "open next daily", "open previous daily"]
        );
        assert_eq!(
            labels(&filter("weekly", READING)),
            vec!["open next weekly", "open previous weekly", "open weekly"]
        );
        assert_eq!(
            labels(&filter("season", READING)),
            vec!["open next season", "open previous season", "open season"]
        );
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
                        && *label != "keep mine"
                        && *label != "take disk"
                        && *label != "undo"
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_active_block_hides_the_caret_commands() {
        let visible = labels(&filter("", READING));
        assert_eq!(visible.len(), COMMANDS.len() - 12);
        assert!(!visible.contains(&"insert link"));
        assert!(!visible.contains(&"follow link"));
    }

    #[test]
    fn the_caret_commands_need_their_block_visible() {
        const EDITING_AT_TABLE: Context = Context {
            block_active: true,
            note_open: true,
            on_table: true,
            sheet_open: false,
            at_bodies: false,
            conflict: false,
            undoable: false,
        };
        const EDITING_AT_SHEET: Context = Context {
            block_active: true,
            note_open: true,
            on_table: true,
            sheet_open: true,
            at_bodies: false,
            conflict: false,
            undoable: false,
        };
        // the logs show the block; the bare table hides it behind the
        // screen; the sheet shows it again
        assert_eq!(labels(&filter("insert", EDITING)), vec!["insert link"]);
        assert_eq!(
            labels(&filter("insert", EDITING_AT_TABLE)),
            Vec::<&str>::new()
        );
        assert_eq!(
            labels(&filter("insert", EDITING_AT_SHEET)),
            vec!["insert link"]
        );
    }

    #[test]
    fn the_finders_hide_off_the_table() {
        assert_eq!(labels(&filter("filter", READING)), Vec::<&str>::new());
        // "jump" now also names the fold row, which is the logs' own
        assert_eq!(labels(&filter("jump", READING)), vec!["fold jump panel"]);
        assert_eq!(labels(&filter("filter", AT_TABLE)), vec!["filter cards"]);
        assert_eq!(labels(&filter("jump", AT_TABLE)), vec!["jump to note"]);
    }

    #[test]
    fn the_folds_hide_on_the_table() {
        assert_eq!(
            labels(&filter("fold", READING)),
            vec!["fold jump panel", "fold rail"]
        );
        assert_eq!(labels(&filter("fold", AT_TABLE)), Vec::<&str>::new());
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
    fn the_conflict_pair_exists_only_while_a_conflict_stands() {
        assert_eq!(labels(&filter("mine", READING)), Vec::<&str>::new());
        assert_eq!(labels(&filter("disk", READING)), Vec::<&str>::new());
        assert_eq!(labels(&filter("mine", CONFLICTED)), vec!["keep mine"]);
        assert_eq!(labels(&filter("disk", CONFLICTED)), vec!["take disk"]);
    }

    #[test]
    fn undo_exists_only_with_something_to_undo() {
        const UNDOABLE: Context = Context {
            undoable: true,
            ..READING
        };
        assert_eq!(labels(&filter("undo", READING)), Vec::<&str>::new());
        assert_eq!(labels(&filter("undo", UNDOABLE)), vec!["undo"]);
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
    fn export_pdf_needs_its_note_on_screen() {
        const CLOSED: Context = Context {
            note_open: false,
            ..READING
        };
        assert_eq!(labels(&filter("export", READING)), vec!["export pdf"]);
        assert_eq!(labels(&filter("export", CLOSED)), Vec::<&str>::new());
        assert_eq!(labels(&filter("export", AT_TABLE)), Vec::<&str>::new());
        assert_eq!(labels(&filter("export", AT_SHEET)), vec!["export pdf"]);
    }

    #[test]
    fn edit_template_hides_on_the_table() {
        assert_eq!(labels(&filter("template", AT_TABLE)), Vec::<&str>::new());
        assert_eq!(
            labels(&filter("template", READING)),
            vec!["edit template"]
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

    /// The palette lists every command alphabetically by label, not in
    /// registration order (`adr/2026-08-palette-order-and-overlay-placement.md`).
    #[test]
    fn the_registry_is_alphabetized_by_label() {
        let labels: Vec<&str> = COMMANDS.iter().map(|c| c.label).collect();
        let mut sorted = labels.clone();
        sorted.sort_unstable();
        assert_eq!(labels, sorted, "the registry must stay alphabetical");
    }

    /// The completeness audit the roadmap demands: the registry against the
    /// chords the app answers. A new chord must touch this list — and its
    /// palette entry — in the same change. Two documented exceptions answer
    /// to no `CommandId` at all, buffer-level like Ctrl+L: Ctrl+T
    /// (`adr/2026-08-ctrl-t-toggles-the-todo.md`) and Ctrl+Shift+D
    /// (`adr/2026-08-delete-note-chord.md`) — the chord and the palette's
    /// "delete note" row both reach `delete_note`, but only the row goes
    /// through this registry.
    #[test]
    fn the_registered_set_matches_the_apps_chords() {
        let chords: Vec<&str> = COMMANDS
            .iter()
            .filter_map(|command| command.chord)
            .collect();
        assert_eq!(
            chords,
            vec![
                "ctrl+f",
                "alt+l",
                "alt+h",
                "ctrl+enter",
                "ctrl+2",
                "ctrl+1",
                "ctrl+l",
                "ctrl+o",
                "ctrl+n",
                "ctrl+d",
                "ctrl+q",
                "ctrl+b",
                "ctrl+shift+f",
                "ctrl+,",
                "ctrl+=",
                "ctrl+-",
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
                "arrange cluster",
                "delete note",
                "edit template",
                "export pdf",
                "keep mine",
                "notices",
                "open loops",
                "open next daily",
                "open next season",
                "open next weekly",
                "open previous daily",
                "open previous season",
                "open previous weekly",
                "open season",
                "open weekly",
                "take disk",
                "toggle theme",
                "undo"
            ]
        );
        let mut names: Vec<&str> =
            COMMANDS.iter().map(|command| command.label).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len(), "labels must be unique");
    }
}
