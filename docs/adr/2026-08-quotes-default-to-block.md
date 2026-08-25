# Every `#quote` renders as a block, never inline

## Context

The template's `show quote.where(block: true): it => block(...)` already drew the muted left border at `width: 100%` — a block quote never had a "natural width" problem, block ones already spanned the full column.
What the show rule's `.where(block: true)` selector left exposed was Typst's own default: a bare `#quote[...]` call, with no `block: true` argument, renders as unstyled inline text that the show rule's selector never matches at all — no border, no spacing, indistinguishable from a stray bracketed span. `>` in the editor already expands to the shorthand (`adr/2026-08-greater-than-expands-to-quote.md`), but nothing forced the call it produces, or one typed by hand, into the block form the rule was written for.

## Decision

`set quote(block: true)` sits in the template's `show` scope, before the `quote.where(block: true)` rule: every `#quote[...]` in a note is promoted to block form before that rule ever sees it, so the border and full-width treatment apply unconditionally rather than only to calls that spelled out the argument themselves.

This is a narrower fix than the todo's own wording ("block quotes take the full width instead of their natural width") suggested — that width behaviour already existed. The real gap was inline quotes never reaching the show rule at all; `set quote(block: true)` closes it by removing the choice, not by changing a width that was already `100%`.

Seeding a fresh vault carries the change automatically (`adr/2026-08-templates-seeded-from-embedded-fixtures.md`'s `include_str!` of this same fixture); an already-seeded vault's `templates/template.typ` is a user-edited file by that same ADR's design and is never overwritten — picking up this default in an existing vault means deleting the stale file so `seed` restores it fresh, the same remediation path that ADR already describes for any template refresh.

## Rejected

- **Pass `block: true` at every `#quote` call site instead** — every note already written, and every future `>`-expansion, would need the argument spelled out by hand; a template default removes the chance to forget it.
- **Auto-migrate existing vaults' `template.typ` on launch** — contradicts `adr/2026-08-templates-seeded-from-embedded-fixtures.md`'s explicit design: the user's edited template always wins over the shipped default, and a template is an editable note like any other.
