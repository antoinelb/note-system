# Repo audit next steps: vault safety, harness, drift, student workflow

## Goal
The 2026-09-02 repo audit's findings are worked off in tier order: the notes cannot be lost to one keystroke, the harness measures what it claims to, the decision records agree with the code, and the features a student hits every week exist — each with an ADR where a decision is made.

## Audit findings (2026-09-02)
Gates on a clean tree: `make static`, `make check-vault`, `make test` (100%, 1169 tests) pass; `make e2e` failed 1 of 7 (`notice-resolves.test.sh`) and passed on rerun.
The failed run's vault held `permanent/onceptevergreen-notes.typ`: the first key after Ctrl+N was swallowed by the overlay's async focus grab (`src/ui.rs:5864` documents the design) and landed in the note behind, in normal mode, where a key is a command.
v0, v1, v2 are closed with ADRs; v3 has no code and six open decisions (`adr/2026-08-v3-mcp-first-order.md`).
`src/main.rs` (247 lines) is outside the coverage gate and holds the `--capture` CLI path, the watcher feed and the `j`/`k` line-walk JavaScript.
No property tests, no CI, seven e2e scenarios, three of which assert on screenshot pixels against `adr/2026-08-headless-x11-e2e.md`.
The real vault at `~/documents/notes` had no version control (fixed by hand on 2026-09-02, git initialised and committed).

## Out of scope
v3 itself — its six open decisions are listed so they can be taken, not taken here.
The v2 ceiling (marks, macros, named registers, visual block, jumplist, configuration) is unchanged.
Spaced repetition and mobile capture: named as gaps, not planned.

## Constraints
Plain `.typ` files remain the source of truth; the index stays derived and rebuildable.
AI never writes prose in note files; no hard blocks; type is a `#meta` field.
Every decision gets its own ADR under `docs/adr/`; when a decision changes CLAUDE.md, both change in the same commit.
Read `.claude/dioxus.md` before touching Dioxus code; invoke the `air` skill before any UI change.
No naked `.unwrap()`, no while loops, colours only in `assets/theme.css`, all UI strings English.
`make test` holds 100% region/line/function coverage when an item is done; window-level fixes get an e2e scenario.

## Items

### Tier 0 — the notes (done 2026-09-02, commit 674f818)
1. Vault backup: a systemd user timer commits the vault every few minutes (`git add -A && git commit`), plus an ADR recording that git is the recovery path for the trashless delete and that the app stays out of it.
2. Overlay focus race: keys typed between an overlay's mount and its async focus grab must not reach the note behind. Fix Ctrl+N, audit the palette, the link picker, the recent-notes picker, the filter and settings overlays for the same race, add a unit test per overlay and one e2e scenario that types the title immediately after the chord, with an ADR.

### Tier 1 — the harness (done 2026-09-02: `src/launch.rs`, `tests/integration/properties.rs`, `harness.sh`, six scenarios, four ADRs; the first property run found three grammar defects, fixed in the same change)
3. `main.rs` inside the gate: narrow `--ignore-filename-regex` to `lib.rs` and `mod.rs`; move the `--capture` path, `watcher_feed`/`feed`, `LINE_WALK` and the hit-probe script into covered modules so `main.rs` holds only the launch builder again (`adr/2026-07-ui-covered-at-100.md`'s promise).
4. Property tests: add `proptest` as a dev-dependency with three properties — no key sequence panics the vim grammar and `u` restores the buffer; markup spans partition the source exactly once; `parse.rs` survives arbitrary input.
5. E2e hardening: move the paced-keystroke helper into `harness.sh`; replace the pixel probes in `notice-resolves.test.sh` and `table-chords.test.sh` with file and index oracles; add a preflight naming a missing `convert` or `sqlite3`; inject today's date instead of reading `date`; retire the clock assertion at `src/time.rs:196`.
6. E2e scenarios: `j`/`k` across a wrapped line (the only thing that sees `LINE_WALK`), delete then undo, an external edit while the app runs, `--capture` from a second process, the ex line, settings.
7. CI decision: an ADR deciding whether the pre-commit hook is the CI or a job runs all four gates including e2e; the hook installed by `make init` is opt-in today.

### Tier 2 — drift (done 2026-09-02: README and roadmap status, seven ADR banners, `todo.md` retired into `2026-09-02-temporal-panes.md`, four ADRs — versioning, `generated/`, no log file, the note-taker practice)
8. README status (v0, v1 and v2 shipped) and CLAUDE.md roadmap status; the v2 line lists "configuration" among the six that stay out.
9. Superseded banners on the seven ADRs superseded one way (`note-history-back`, `ctrl-q-flushes-then-closes`, `cursor-always-in-the-note`, `status-surface-owns-notices`, `watcher-feeds-the-ui`, `escape-ladder-editor-wide-mode`, `plan-realigned-with-wireframes`) and a lift note on `2026-07-logs-centre-read-only.md`'s "until phase 8".
10. `todo.md`: the three temporal-pane items become a plan or are dropped; the foreign `data/cache/` line leaves `.gitignore`.
11. Missing ADRs: versioning (what `0.2.0` means against v0–v3), the `generated/` category (its template and who may write there), error logging, the note-taker practice.

### Tier 3 — student workflow, by weekly hit rate
12. `$` joins the autopair table (`src/editor.rs:832`), amending `adr/2026-08-autopairs-in-the-typing-path.md`.
13. Full-text search: an FTS5 table beside `notes` in the index, a palette row to query it.
14. Export: a palette row writing the open note to PDF beside it via `RenderTheme::Paper`.
15. Equation latency: stale-while-revalidate for fragments so a changed formula keeps its last image until the compile lands.
16. Course workflow: a `course` type and lecture template, a due-date convention surfaced as a loop family (amends the "no ages, no grouping" clause of `adr/2026-07-debt-counter-then-list.md`).
17. A link form that is not a note id, so slide decks and PDFs stop manufacturing dangling debt.
18. Images: clipboard image paste into `vault/assets/`, the first non-`.typ` files in the vault.

### v3 — decisions to take before phase 1
The serialization dependency for the MCP server; the suggestion record shape and store; whether the server reads the index or files only; what the `claude` CLI is asked and what a failure becomes under "no hard blocks"; the tag-confirm accept path; the dashed-edge and proposal-line surfaces.

## Acceptance
Per item: `make static && make test` green at 100%; `make e2e` green including any new scenario, which fails against the pre-fix binary.
Every decision made along the way has an ADR; CLAUDE.md is updated where an invariant or the roadmap changes.

## Check
`make static && make test`
