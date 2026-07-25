# Two screens (the table and the logs), and the table stays v1

## Context

`docs/design/` holds the first design pass: wireframes exploring six app shells (`1a`–`1g`), six table treatments (`2a`–`2f`) and six riffs (`3a`–`3f`), distilled into `ui-spec-table-and-logs.md` from `3a` (table + card writing) and `3e` (logs).

Two mismatches with the plan had to be settled.
The wireframe file is titled *v0*, but `plan.md` § Roadmap puts the canvas in v1.
And roadmap phase 4 plans a note list as the app shell, while no list appears anywhere in the design.

## Decision

The app has exactly **two screens**, with a button each way: the **table** (spatial canvas of permanent notes) and the **logs** (day, week and season notes on one screen).
Time notes never appear on the table.

- **The table stays v1.** Nothing in the v0 goal — "daily driver for writing" — needs the canvas, and the table's writing sheet *contains* the editor, so it could not precede the editor phases anyway. Keeping it in v1 also keeps `plan.md` § Known risks 4 (scope creep at v1) contained.
- The wireframes are therefore the **v0 + v1 target UI**, not a v0 specification. They govern every v0 choice that touches both screens: greyscale chrome, a single alert hue, type colour only on cards, rendered typst never restyled by the app.
- **The logs screen becomes a v0 phase of its own.** It needs only rendering, navigation and create-from-template, so it lands right after the editor phase.
- Consequently **weekly and season notes move into v0** — they were "later" in `plan.md` § Time-based notes, but the logs screen shows all three scales side by side.
- **Phase 4's note list is scaffolding.** Its job is to de-risk the embedded typst `World`, not to be UI; it is deleted when the logs screen lands.

## Consequences

The split-view fallback declared in `plan.md` § Known risks 1 is unusable inside a ~600 px writing sheet (two ~300 px panes).
Because the sheet arrives with the table in v1, that tension defers with it: v0's editor is full-window, where split view still works.

## Alternatives rejected

- **Build the table in v0, as the wireframe title implies** — adds a full phase (durable positions, pan, two zoom levels, edges, the tethered sheet) before daily-driving starts, and it cannot be built before the editor exists regardless.
- **A rough titles-only table in v0, polish in v1** — the cheap half is pan and cards; the half that makes the table worth having (durable positions, the tethered sheet) is the v1 work either way, so the split buys a demo rather than a tool.
- **Keep the note list as a permanent third screen** — the index queries behind it stay useful, the screen does not; the logs and the table cover navigation between them.
