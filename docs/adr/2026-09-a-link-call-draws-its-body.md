# A `#link` call draws its body, and the call around it is a delimiter

## Context

`adr/2026-09-link-is-for-resources.md` gave Typst's own `#link(dest)[body]` the same `Role::Link` a note link wears, which stopped a line holding one from falling back to a compiled widget.
It gave it that role as *one span over the whole invocation*, so an inactive block read `#link("/assets/slides.pdf")[the slides]` where the compiler draws `the slides` — the exact mismatch `adr/2026-09-inactive-blocks-hide-their-syntax.md` was written to remove for the heading's `=`, the list's `-` and the quote's `> `.
That ADR's own Consequences accepted the mismatch for calls, on the grounds that "a link's rendered text is the compiler's to decide, not a span CSS can hide".
It is not: the body of a `#link` is a `ContentBlock` in the parse tree, its two brackets are leaves, and everything between them is the markup Typst will draw and nothing else.
The `[[id]]` run next to it had already been split into three spans — `[[`, the id, `]]` — with the pairs flagged as delimiters (`adr/2026-09-wiki-links-replace-the-l-call.md`); the resource link was the one link form still showing its plumbing.

## Decision

**A `#link(dest)[body]` is three spans, all `Role::Link`.**
`#link(dest)[` and the closing `]` are flagged `delimiter`, so `.block-css .mk-delim` hides them on an inactive block with the mechanism that already exists — no new CSS, no new role, no new class.
The line under the caret and a selected line show the whole source, as every construct does.

**The body's own children stay on the walk.**
`push_call` pushes the content block's inner nodes back onto the span walk with the link's role inherited, rather than emitting one flat span for the body: a `*strong*`, an `_emph_`, a nested `[[id]]` inside a link body keeps its own role and its own delimiters, and the body's plain prose takes the link colour.
The spans still tile the block byte for byte — the invariant the hit probe's `data-start` walk and `tests/integration/properties.rs` both read.

**A call with no body keeps its whole source on screen.**
`#link("https://example.org")` has no content block, and `#link(dest)[x` still being typed parses with an `Error` leaf where its `[` belongs; both fall back to the single opaque span the call had before this change.
Hiding a prefix with nothing behind it would erase the line from the note, which is a worse answer than showing the source — the same forgiving posture the `Error` exception takes everywhere else in `markup.rs`.

**`#meta` and `#quote` are untouched.**
Only `Role::Link` reaches the body path: those two are opaque calls whose arguments the compiler owns, and neither has a body a reader is meant to see instead of the call.

Nothing about following a link changes: `links::link_at` reads the destination off its own parse of the block text and never looks at a span, so `gf`, Ctrl+Enter and the palette open the same resource they did before.

## Alternatives rejected

- **One flat `Link` span for the body** — simpler by two lines, and it would flatten a `*bold*` or a nested `[[id]]` inside a link body into undifferentiated link text; the walk already knows how to descend, so declining to is a loss with no saving.
- **A `Role::LinkDest` for the hidden half** — a second role and a second class for something `mk-delim` already hides exactly right, and it would break the rule that a delimiter keeps its run's own role so the run never changes weight mid-way.
- **Hiding the dest of a bodyless `#link` too, drawing the URL alone** — the URL is the argument, not a body; slicing a string literal out of the call to show as prose is a rendering the parse tree does not name, and the reader loses the ability to see what they typed.
- **Rewriting the source to `[[…]]`-like sugar for resources** — `#link` is vanilla Typst and stays vanilla Typst (`adr/2026-09-link-is-for-resources.md`); this is a rendering decision, not a syntax one.

## Consequences

Entering a line that holds a `#link` now moves its text, as entering a heading or a list item already does.
That stays within "nothing moves unless the user moved it": the caret's own move is the only trigger, and it is the consequence `adr/2026-09-inactive-blocks-hide-their-syntax.md` already accepted for every other prefix.

This supersedes that ADR's Consequences line reading "`#l(…)`, `#meta(…)` and every other call still show their source on inactive lines".
`#meta` and `#quote` still do; `#link` does not.
