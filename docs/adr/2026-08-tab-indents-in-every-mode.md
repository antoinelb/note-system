# Tab indents the line, in every mode but R

## Context

The editor had no notion of indentation at all: nesting a Typst list meant typing spaces.
Tab was dead everywhere — `Action::Ignore` in the phase-0 keymap, `Outcome::Swallow` in normal, visual and replace, `Outcome::Pass` in insert — consumed only so the browser default would not walk focus out of the invisible sink (`adr/2026-08-hidden-ime-sink.md`).

vim's own indent keys are `>>` and `<<`, which the v2 ceiling never included.

## Decision

- **One indentation level is two spaces**, `caret::INDENT`, Typst's own nesting width for markup lists.
  It is defined once and shared by the grammar's Tab and the editor's list continuation (`adr/2026-08-list-continuation-on-enter.md`), because a drift between the two would nest lists the continuation could not then continue.
- **Tab indents the whole line whatever column the caret sits at**, and Shift+Tab dedents it; the caret rides the shift rather than jumping to the first non-blank as vim's `>>` does.
  Mid-word Tab moving the line — not inserting whitespace at the caret — is the one rule, in every mode, with no caret-column special case.
- **Visual mode moves every selected line and spends the selection**, landing in normal exactly as vim's `>` does.
- **Replace mode keeps swallowing Tab.** `R` overwrites cluster by cluster against a session start; the line moving underneath would desync what Backspace restores (`adr/2026-08-replace-mode-session-and-backspace.md`).
- **Insert mode gains its second grammar key.** The contract was "phase 0's writing flow, untouched — only Escape is the grammar's"; Tab joins Escape, handled in `Vim::handle` above the mode dispatch so all three modes share one implementation.
  Its splice lands at the line start, which can sit before the insert session's own start, so `insert_from` rides the same shift its text did — otherwise the dot would replay a truncated session.
- **A press with nothing to move swallows rather than checkpointing an empty change**: a bare line dedenting, a blank line either way.
  Blank lines inside a selection stay blank, as vim's `>` leaves them.
- **A stray Tab behind an armed verb aborts it**, as every other key that cannot be its noun does; there is no `3<Tab>`.
- **One splice, so one checkpoint**, and a single `u` reverses a whole multi-line indent.
  Because only row starts are edited, the bytes between rows — newlines and the block separators no line owns — ride along untouched, so a selection spanning blocks needs no special case beyond what `Act::Splice` already routes (`adr/2026-08-editor-splice-cross-block.md`).
- **Plain Tab never reaches the phase-0 keymap any more**; `Action::Ignore` survives as the net for the Tab chords the grammar passes on (Ctrl+Tab), which would otherwise walk focus out of the sink.
- **v3's ghost text takes precedence over the indent.** CLAUDE.md's AI invariant gives Tab to accepting a suggested link; when a ghost suggestion is showing, Tab accepts it, and indents otherwise — the completion rule every editor with both already uses. Recorded now so v3 does not rediscover the collision.

## Rejected

- **`>>` and `<<` instead of Tab** — vim's own keys, but they work in normal mode only, and the friction is at its worst while writing a list in insert mode.
- **Insert-mode Tab inserting whitespace at the caret** (vim's default) — a second rule for the same key, and the line-moving meaning is the one being asked for.
- **Handling Tab in `keymap.rs` for insert and in `vim.rs` for the rest** — two implementations of the same arithmetic, free to drift.
- **Putting the caret on the first non-blank after the shift** (vim's `>>`) — correct in normal mode, but it would yank the caret out of the word being typed in insert mode.
- **Keeping the selection alive after a visual Tab** so it can be pressed repeatedly — vim spends it, and `.` already repeats a change.
- **Deleting `Action::Ignore`** — Ctrl+Tab passes the grammar by before the mode dispatch and still needs swallowing.
