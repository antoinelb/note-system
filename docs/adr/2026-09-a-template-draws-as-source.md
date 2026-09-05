# A template's code blocks draw as source, never as a compiled widget

## Context

Templates open in the one editor (`adr/2026-08-template-editing-in-the-one-editor.md`), and typing in one flashed: every few hundred milliseconds most of the page dimmed to raw text and lit back up.

The cause is two decisions meeting, each right on its own.
A block whose parse tree holds a `ModuleImport`, a `ShowRule` or a `#let` is not markup the CSS model owns, so `markup::model` answers `Draw::Typst` and the block draws through `FragmentCache` as a compiled SVG (`adr/2026-08-css-draws-the-markup.md`).
And every autosave of a template fires `VaultChange::Template`, on which the shell runs `fragments.borrow_mut().clear()` — the template is the one compile input a fragment key never carries (`adr/2026-08-template-touch-clears-caches.md`).
`FragmentCache::clear` empties `entries` *and* `shelves`, so the shelf that normally holds a changed block's last image (`adr/2026-09-fragments-shelve-their-last-svg-per-block.md`) has nothing to hold: every fallback block on screen answers `Pending { shelved: None }` and draws `.pending-source`, its dimmed raw text, until the recompile lands.
The autosave's quiet window is 500 ms, so continuous typing rebuilt that cycle continuously.

The compile it was paying for produced nothing.
A template's non-markup blocks are `#import`, `#show: note`, `#meta(id: "{{id}}", …)`, and in `template.typ` a page of `#let`: the first three render an empty page by construction, and `{{id}}` is a placeholder that is not a value in any note.
The prior ADR already said as much — "its non-import blocks render near-empty pages" — and accepted the churn as "the price of correctness".
There is no correctness being bought: the pixels the clear invalidates are pixels of nothing.

## Decision

**While the one editor holds a file under `templates/`, a `Draw::Typst` block draws as plain source text instead of a compiled widget.**
`block_panes` derives this from the buffer's own path — `open_template`, the same derivation "template mode" already is, no new state — and hands the block to `Pane::Css` with `markup::plain`: one `Role::Text` span tiling the block's content, `BlockRole::Plain`.
Nothing about the fallback machinery changes; a template simply never enters it, so a template's autosave clears a cache no block on screen was reading and the flash has no surface left to happen on.

Blocks a template holds that CSS already owns are untouched: `= {{id}}`, `== Notes`, `- [ ]` still draw as heading, heading and checklist, so a template still previews the shape of the note it makes.

`markup::plain` lives in `markup.rs` beside `model` because the tiling invariant it must keep — every byte in exactly one span, ascending, no gaps — is that module's, and `plain` is the degenerate case of it.

The change is also the layout answer. `.block-css` and `.block-active` carry a byte-identical block box (`assets/theme.css`), so a template's code line is now exactly as tall inactive as it is under the caret; the SVG it used to swap with was neither.

## Alternatives rejected

- **Teach the template's own autosave not to clear the caches** — the seam idiom of `adr/2026-09-the-app-indexes-its-own-writes.md`, applied to the caches instead of the index. It is the larger change: `VaultChange::Template` is a unit variant, so it would have to start carrying a path, and the shell a ledger of its own writes to compare against. It is also incomplete twice over. Editing `template.typ` itself genuinely does invalidate every compile in the vault, so the ledger would have to except the one template most worth editing — and that is the template whose blocks are all `#let`, the flashiest file of the thirteen. And it treats the symptom: the app would keep compiling `#let palette = (` into an empty page, just less often.
- **Widen the CSS allow-list to cover `#let`, `#import` and `#show`** — the allow-list is deliberately the three calls the app owns semantically, and this would be the app claiming it can style arbitrary Typst code. Drawing code as plain text is not a claim about how code should look; it is the absence of one.
- **A separate template renderer** — the prior ADR already rejected a second editor surface, for the same reason.
- **Debouncing the clear, or shelving across it** — later pixels or wrong pixels, for a compile whose product is a blank page either way.

## Consequences

`templates/template.typ` — the one template that is nothing but code — used to open as a page of `cyclic import` errors, one per block: the fragment compiles a block of `template.typ` against `template.typ`.
It now shows its own source, which is the only thing it was ever going to be read as.

A template's preamble now shows its three lines of source rather than the near-empty page they compiled to, which is what the block shows the moment the caret enters it anyway: inactive and active finally agree.
`adr/2026-08-template-editing-in-the-one-editor.md`'s last Decision paragraph — "churny once per 500 ms quiet window, accepted as the price of correctness" — is superseded; the clear still happens, and is now invisible.
`adr/2026-08-css-draws-the-markup.md`'s "a block's own parse-tree verdict decides which renderer draws it" gains one condition ahead of it: in a template, no block is drawn by the compiler at all.

There is no e2e scenario. The vault and the index are the same after this change as before it — the flash is pixels, and `make e2e` never asserts on pixels (`adr/2026-08-headless-x11-e2e.md`); `edit-template-from-table.test.sh` already covers a template opening and taking an edit, and still passes. What witnesses the change is a ui test: the same preamble takes a fragment cache entry in `permanent/` and none in `templates/`. The flash was measured out of the app by hand, recording the headless window at 30 fps while a sentence was typed into `templates/daily.typ`: before, ten frames in fourteen seconds showed the preamble collapsed to dimmed source and the rest of the page displaced ~190 px; after, all 420 frames are the same page.
