# On any UI conflict, the wireframes win — plan.md realigned

## Context

`docs/plan.md` § Screens, § Canvas, § Friction and § AI were written before the six-turn design pass, and were never re-read against its result.
An audit of `plan.md` and `roadmap-v0.md` against `design/wireframes-v0.md` Part I found seven statements the design had retired and three requirements the design carries that neither document tracked.

The retired statements are all traceable to a specific turn that superseded them:

| plan.md said | the design says | superseded by |
|---|---|---|
| "Chrome is greyscale throughout" | indigo void `#0d0b18`, ink `#c9c4dd`, link spans `#a99ad9`, amber ember `#d9b06a` | turn 4's mood pass (4c "Deep field") |
| jump panel has "week and season **chips**" | clickable week-number gutter and a season row; *"no borders, no chips, no kbd hints anywhere"* | turn 5 (5b "One line") |
| sheet has "id and type in a thin header … link counts in the footer" | *"the **in-note** meta line at the top, and a footer showing **only backlinks**"*; *"the sheet shows the note's own title"* | turn 5, and the card-header idea dropped after turn 3 |
| debt items listed "(with age)"; "ages visibly" | the loops list has no ages; age appears on *table cards* ("capture · 3 d") | `adr/2026-07-debt-counter-then-list.md` |
| season id `2026-été` | rail rows read `2026-summer · season`, and rail labels are ids | turn 6 + `adr/2026-07-repo-language-english.md` |
| "**clicking** an empty day offers to create it" | *"press **enter** to start one … **Only enter** creates the file"* | turn 6, state `6d` |
| suggestions surface as a "suggestion list per note" | *"one dashed line at the end of the page"*, hint "enter accept · x dismiss" | turn 6, states `6b`/`6e` |

The three untracked requirements: light mode (see `adr/2026-07-design-language-own-phase.md`), the **"captured today"** block under the day note, and the fact that the design's prose typography is a `template.typ` requirement rather than an app one.

## Decision

`plan.md` is corrected on all seven points and gains the three requirements, and the § Screens section now carries the precedence rule explicitly: **on any conflict between the plan and the wireframes, the wireframes win.**
The plan describes *what and why*; the wireframes are the frozen *what it looks like*, and only the latter was produced by a deliberate iteration with a recorded trail.

Two consequences worth naming, because they change work rather than prose:

- The **`2026-summer` id retires the accented-id coverage** that `adr/2026-07-repo-language-english.md` had earmarked for phase 5.
  What a season *is* (its boundaries) remains an open phase-5 ADR; only the id form is now settled.
- The **phase-5 split-view editor is scaffolding**, and the roadmap now says so.
  `plan.md` § Known risks justified the split-view fallback on the grounds that "the editor is full-window" in v0 — but the design's v0 screen is three panes, so the v0 editor is a centre column.
  Split view fits neither that column nor v1's writing sheet; it exists so writing can start before the real screen does, and is deleted in phase 7 (the buffer under it survives).
  The hybrid-editor fallback is correspondingly restated: a single pane toggling source ⇄ rendered, not two panes.

## Alternatives rejected

- **Treat the plan as authoritative and revise the design** — the wireframes are a frozen artifact with a six-turn trail behind every choice (`wireframes-v0.md` Part II); the conflicting plan sentences have no recorded reasoning at all, they simply predate the pass.
- **Leave the stale sentences and rely on the design doc's precedence** — the plan is named the *"standalone source of truth"* in `CLAUDE.md` and is what gets read before writing code. A source of truth with seven known-false statements in it is worse than one that has been corrected.
- **Correct only the sentences that block phase 5** — the drift was invisible precisely because nobody re-read the whole section; a partial pass guarantees the next phase rediscovers the rest.
