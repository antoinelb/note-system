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
}

/// One palette row: the plain English name a command is found by, and the
/// chord it also answers to — `None` for the mouse-only commands.
#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub id: CommandId,
    pub label: &'static str,
    pub chord: Option<&'static str>,
}

/// The birth command list (`adr/2026-08-palette-birth-command-list.md`),
/// in the order the palette shows it.
pub const COMMANDS: [Command; 9] = [
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
];

/// What was true when the palette opened — decides which commands exist at
/// all. One flag: the caret commands need a block to act on, and the caret
/// itself is probed at run time, never to decide visibility.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Context {
    pub block_active: bool,
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
        _ => true,
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const EDITING: Context = Context { block_active: true };
    const READING: Context = Context {
        block_active: false,
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
        assert_eq!(
            labels(&filter("", EDITING)),
            COMMANDS.iter().map(|c| c.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn no_active_block_hides_the_caret_commands() {
        let visible = labels(&filter("", READING));
        assert_eq!(visible.len(), COMMANDS.len() - 2);
        assert!(!visible.contains(&"insert link"));
        assert!(!visible.contains(&"follow link"));
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
            ]
        );
        let chordless: Vec<&str> = COMMANDS
            .iter()
            .filter(|command| command.chord.is_none())
            .map(|command| command.label)
            .collect();
        assert_eq!(chordless, vec!["open loops", "go to today"]);
        let mut names: Vec<&str> =
            COMMANDS.iter().map(|command| command.label).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len(), "labels must be unique");
    }
}
