# Tier 3: the student workflow, and the temporal panes

## Goal
The seven tier-3 items of `2026-09-02-repo-audit-next-steps.md` and the two items of `2026-09-02-temporal-panes.md` are shipped, each with its tests, its e2e scenario where the window is involved, and its ADR.

## Decisions already taken (with the user, 2026-09-02)
Links to things that are not notes use vanilla Typst `#link("…")[text]`; `#l("id")` stays for notes only.
The course workflow is one new permanent type, `course`; a lecture is a `source` note linking its course; a `due: "YYYY-MM-DD"` `#meta` field on any note feeds two new loop families.
An image reaches a note through normal-mode `p`/`P` and insert-mode Ctrl+V when the clipboard holds an image and no text.
Full-text search opens on Ctrl+Shift+F and its results open like the Ctrl+B picker: a `time/` note on the logs, anything else in a sheet.

## Out of scope
v3 (AI) and its six open decisions. The v2 ceiling (marks, macros, named registers, visual block, jumplist, configuration). Spaced repetition, mobile capture. Opening a `#link` target from the e2e harness (no browser in Xvfb; the index is the oracle there).

## Constraints
Plain `.typ` files remain the source of truth; the index stays derived and is rebuilt by bumping `SCHEMA_VERSION` (`adr/2026-07-disposable-index-user-version.md`) — never migrated in place.
Every note still compiles with the vanilla typst CLI: a new `#meta` field means `templates/template.typ`'s `meta` signature grows with it, and `make check-vault` proves it.
AI never writes prose in note files; no hard blocks; type is a `#meta` field; the eight-type closed set becomes nine everywhere it is enumerated (`create::TYPES`, the type bars, the filter, `is_permanent`).
Invoke the `air` skill before any UI change; read `.claude/dioxus.md` before any Dioxus code. Nothing on screen moves except what the user moved; every input is acknowledged within 100 ms, so compiles and exports run on the compute tier or a thread, never on the UI thread.
No naked `.unwrap()`, no while loops, no recursion, no colour literal outside `assets/theme.css` (both themes filled together), spacing in multiples of 4, all UI strings English, one sentence per line in `.typ` and `.md`.
Long writes are atomic: temp file then rename (`persist.rs` has the seam).
Every decision gets an ADR under `docs/adr/` in the same change; CLAUDE.md changes where an invariant or the roadmap changes.
`make test` holds 100% region/line/function coverage when an item is done; window-level behaviour gets a scenario under `tests/e2e/` asserting on `.typ` files and the index, never pixels, paced with `e2e_key_paced` across focus grabs.
Commits go through the `commit` subagent (haiku) with an explicit path list; never `--no-verify`.

## Items
1. `$` joins `editor::PAIRS`: typing `$` opens `$$` with the caret inside, a second `$` steps over the closer, and the apostrophe guard does not apply. Amend `adr/2026-08-autopairs-in-the-typing-path.md` with a short ADR recording why `$` earns its way in (equations in course notes) where `*` and `_` did not.
2. Full-text search: an FTS5 virtual table (`notes_fts`: path, title, body) beside `notes`, filled on every survey and single-note index and emptied on delete, rebuilt with the schema bump; `Index::search(query, limit) -> Vec<SearchHit { path, title, snippet }>` using `snippet()`, tokenizer `unicode61 remove_diacritics 2` so `idee` finds `idée`. A Ctrl+Shift+F overlay shaped like the Ctrl+B picker (`adr/2026-08-ctrl-b-recent-notes-picker.md`): query, ranked rows with the snippet, arrows, Enter opens by the category rule, Escape closes; a palette row "search text". Relayed keys obey `adr/2026-09-overlay-keys-relay-before-focus-lands.md`. ADR.
3. PDF export: `typst-pdf` joins the dependencies; `render::ExportJob` compiles the note with `RenderTheme::Paper(DEFAULT_SIZE)` on the compute tier (`compute::Job::Export`, `Outcome::Export`) and writes `<note>.pdf` beside the `.typ` atomically; the palette row "export pdf" is offered whenever a note is open (sheet or logs selection); the landing is a notice naming the path, a failure a notice with Typst's first diagnostic. `*.pdf` is already ignored by git. ADR.
4. Equation latency: `FragmentCache` keeps a shelf per block slot — `probe` takes the block's index in the note and, while the new content key is pending, answers `Pending { job, shelved: Some(svg) }` with the last ready SVG that slot showed; the editor draws the shelved image with a `stale` class (dimmed, same metrics) instead of dimmed source, and the fresh compile replaces it when it lands. `sweep` and `clear` drop shelves with their entries. Amends the "fragments need no staleness guard" bullet of `adr/2026-08-async-caches-pending-stale.md`. ADR.
5. The `course` type and the `due` field: `NoteType::Course` is the ninth permanent type with `templates/course.typ` (title, then two prose lines for the code and the term — no new placeholders, `adr/2026-07-template-placeholders-closed-set.md`), `--bar-course` in both themes, a row in `create::TYPES` and every enumeration test. `#meta(due: "YYYY-MM-DD")`: `template.typ`'s `meta` accepts and prints it ("due 2026-09-10" in the meta line), `parse::extract_due` fills `Meta.due` or a `MetaAnomaly::InvalidDue(raw)`, `notes.due` in the index, `Index::due_notes(today)` returns overdue and due-within-7-days rows, and `loops::lines` gains the families "overdue since <date>" and "due <date>", each line opening its note. Amends the "no ages, no grouping" clause of `adr/2026-07-debt-counter-then-list.md`. The fixture vault gains one course note and one note with a past `due` so the e2e and the index tests see them. ADR.
6. External links: `links::link_at` recognises a `#link("target")` call under the caret alongside `#l`; `follow_at` opens a `link` target with `xdg-open` (spawned detached, never awaited on the UI thread), resolving a leading `/` against the vault root so `#link("/assets/slides.pdf")[slides]` opens the file; a launcher that fails is a notice. The parser keeps ignoring `link` calls, so nothing here is dangling debt; the fixture vault gains one `#link` and `make check-vault` proves it compiles. ADR.
7. Images: `arboard` gains `image-data`; the clipboard worker gains `read_image() -> Result<Option<Vec<u8>>, String>` returning PNG bytes (encoded with the `png` crate in the worker, never on the UI thread); `template::seed` creates `vault/assets/`; the watcher already ignores non-`.typ` files. Normal-mode `p`/`P` (`Act::Paste`) and insert-mode Ctrl+V: when the text read is empty and an image is present, the PNG is written atomically to `assets/<note-stem>-<yyyymmdd-hhmmss>.png` and `#image("/assets/<file>")` is inserted at the caret (after or before it for `p`/`P`); text keeps today's path. `--capture` is untouched. ADR.
8. Temporal panes: `.rail` narrows from 208px to the widest date row plus a 16px gutter each side, measured on the fixture vault; Alt+H folds the rail and Alt+L the jump panel, both toggles on the logs screen and palette rows ("fold rail", "fold jump panel"), a folded pane leaving its hairline; the chords are guarded to plain Alt (no Ctrl, no Meta) and to the logs pane so AltGr compositions in insert mode are untouched. Session-only state. ADR.
9. E2e scenarios, one per window-level item, each failing against the pre-change binary: `search-text` (Ctrl+Shift+F, a word from a fixture note, Return, a typed sentence lands in that note), `export-pdf` (palette row over an open sheet, the `.pdf` appears beside the note and is non-empty), `image-paste` (`xclip -selection clipboard -t image/png` seeds the clipboard — `xclip` joins the preflight — then `p` writes `assets/*.png` and the note holds `#image("/assets/`), `course-and-due` (Ctrl+N "course" writes `permanent/<title>.typ` with `type: "course"`; the open-loops overlay's due line opens the fixture's overdue note and a typed sentence lands there), `fold-panes` (Alt+H, Alt+L, then `i` and a sentence still land in the day note).
10. ADRs for every item above, CLAUDE.md updated where the invariants change (nine types, the `due` field, `#link` for resources, `vault/assets/` as the first non-`.typ` files), and `2026-09-02-repo-audit-next-steps.md` and `2026-09-02-temporal-panes.md` marked done.

## Acceptance
`make static && make check-vault && make test` green with coverage at 100%.
`make e2e` green including the five new scenarios.
Every item has its ADR; CLAUDE.md names the nine types, the `due` field, `#link` and `vault/assets/`.

## Check
`make static && make check-vault && make test`
