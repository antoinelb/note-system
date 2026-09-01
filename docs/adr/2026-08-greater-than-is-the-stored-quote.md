# `>` is the stored quote syntax, taught to Typst by the template

## Context

`adr/2026-08-greater-than-expands-to-quote.md` gave `> ` an Enter-time rewrite: type `> quotation`, press Enter, and the editor spliced the line into a native `#quote(...)[...]` call before the newline landed, so the file never held `> ` at rest — only the expanded call. That ADR explicitly rejected the alternative of storing `> ` literally and preprocessing it before render, on the grounds that it "would give the app and vanilla Typst different meanings for the same source": a file read by this app and the same file read by `typst compile` would show different things unless the app's own preprocessing ran first.

`adr/2026-08-css-draws-the-markup.md` now needs a role to give `>` inside the CSS-drawn markup model — a quote line has to be recognisable as *itself* in the block's stored text, the way a heading's `=` or a list's `-` already is, for `markup::model` to classify the block `BlockRole::Quote` and tag its leading `"> "` a `Role::Marker` span rather than plain text. A `>` that only ever exists transiently, rewritten away by the time the file is saved, has no stored form for a parse-tree-driven model to recognise at all.

## Decision

**`>` becomes what the file stores.** A line beginning `> ` is written to disk exactly as typed, byte for byte, with no Enter-time rewrite. The Enter-time splice in `src/editor.rs` that used to convert it into `#quote(...)[...]` is deleted outright — `greater-than-expands-to-quote.md`'s gate (a collapsed caret at a physical line's end) and its splice-through-`Editor::splice` mechanism both go with it.

**Vanilla Typst is taught to read it, not preprocessed around it.** `templates/template.typ` gains a `show par:` rule that recognises a paragraph starting with `>` and lays it out as a block quote — the same visual treatment `greater-than-expands-to-quote.md` specified (muted left rule, inline italic attribution off a trailing ` _…_` span) — entirely inside Typst's own show-rule machinery. Nothing outside the compiler touches the text before it reaches this rule: `typst compile some-note.typ` on the vanilla CLI, with no app in the loop at all, renders the quote correctly, because the rule that reads `>` ships in the template every note already imports.

**This is not the rejected alternative.** `greater-than-expands-to-quote.md` rejected "keep `> ` in files and preprocess before render" because *the app's own preprocessing* was the thing giving `>` its meaning — a step outside Typst, run only when this app happened to be the one rendering. What is decided here has no preprocessing step anywhere: the `show par:` rule *is* Typst reading `>` as Typst, the same way it already reads `=` as a heading and `-` as a list marker. There are not two meanings for one source, because there is only one reader that ever assigns `>` a meaning — the compiler — and it does so identically whether invoked by this app's embedded compiler, the vanilla CLI on export, or `BodyCache` compiling a card body. The objection the old ADR raised does not apply to this decision; it would only apply if the app still rewrote `>` before Typst saw it, which it no longer does anywhere.

**Existing `#quote(...)` notes keep rendering, and there is no vault migration.** The template keeps `show quote.where(block: true): …` and `set quote(block: true)` (`adr/2026-08-quotes-default-to-block.md`) exactly as they stand, alongside the new `show par:` rule — a note written under the old Enter-expansion behaviour, holding a literal `#quote(...)[...]` call, compiles exactly as it always did, because that rule was never touched. The two rules simply produce the same visual quote from two different stored spellings; nothing needs to be rewritten in any file already on disk.

## Rejected

- **Keeping the Enter-time expansion and adding `>` as a second, redundant spelling** — rejected as pure duplication: the CSS markup model needs a stored form to key a role off, and once `show par:` can read `>` directly, there is no remaining reason for the editor to also rewrite it into the call it already renders as.
- **Teaching `markup::model` to expand `>` into a synthesized `#quote` role at render time, keeping the file's stored form ambiguous** — rejected because it re-introduces exactly the two-meanings problem `greater-than-expands-to-quote.md` warned about, just moved from "the app's Enter handler" to "the app's markup renderer": the file would still mean one thing to this app's CSS path and a different, unstyled thing to the vanilla CLI unless the CLI also had a rule for it — which is what the `show par:` rule now provides directly, making a second, app-only interpretation unnecessary.

## Consequences

`adr/2026-08-greater-than-expands-to-quote.md` is superseded by this decision; its Rejected section's objection to storing `>` literally is answered above rather than reopened.
`adr/2026-08-list-continuation-on-enter.md` cited the old Enter-time expansion as precedent for its own gate and cited an ordering between "the quote shorthand" and list-marker continuation at Enter; that ordering no longer exists as live code now that nothing expands `>` at Enter, and that file carries an amendment note to that effect rather than being rewritten.
`adr/2026-08-quotes-default-to-block.md` needs no change: its `set quote(block: true)` decision governs every `#quote(...)` call regardless of which rule produced it, and both rules that can produce one — the old literal call and the new `show par:` rule — still route through it.
