# Greater-than lines expand to native Typst quotes

**Superseded by `adr/2026-08-greater-than-is-the-stored-quote.md`**: `>` is now what the file stores at rest — the Enter-time expansion below is deleted, and a `show par:` rule in `templates/template.typ` teaches vanilla Typst to read `>` directly, so there is no longer a preprocessing step for the app and the compiler to disagree about.

## Context

Quotes are common in time notes, but Typst's `#quote(block: true)[…]` is expensive to type repeatedly.
Markdown's `> ` gesture is familiar, while storing it literally would violate the rule that every note compiles with the vanilla Typst CLI.
The shared template already owns note typography across the app and exports.

## Decision

In insert mode, Enter at the end of a collapsed line beginning exactly with `> ` replaces that line with a native block quote and inserts the requested newline.
Each completed line becomes its own quote.
A non-empty trailing ` _…_` span becomes the quote's attribution; earlier emphasis remains body markup.
Incomplete forms and presses away from the line end retain ordinary newline behavior.
The conversion belongs to the current insert intent, so the existing editor history reverses it with the surrounding typing.

The shared template shows native block quotes without quotation marks, using a muted left rule and inline italic attribution.
Both the stored source and every renderer therefore use Typst's `quote` element.

## Rejected

- Keeping `> ` in files and preprocessing before render would give the app and vanilla Typst different meanings for the same source.
- Converting as soon as Space follows `>` would expose generated delimiters before the quotation is written.
- Grouping consecutive lines would delay conversion and make the boundary depend on a later blank line.
- Treating every emphasized span as attribution would remove ordinary emphasis from quoted prose.
