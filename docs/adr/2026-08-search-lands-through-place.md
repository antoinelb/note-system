# / searches the visible lines and lands through `place_at`

## Context

Phase 5: `/` search within the note, `n`/`N` to walk matches; a match may live in a rendered block, so landing must activate it — the offset-walking cousin of Ctrl+Enter's `links::link_at`.

## Decision

- **`motions::search`**: plain-text matching over the *visible lines* — a match cannot straddle blocks or hide in a separator — char-wise with **smartcase** (an all-lowercase pattern matches any case; a capital anywhere makes it exact — the right default for French), accents exact, **wrap-around** both ways.
- **The prompt is one line**, the picker's little sibling, mounted beside it on both surfaces: `/` in normal mode emits `OpenSearch`, the widget opens the input; Enter commits the pattern into the grammar (`Vim::commit_search`) and the first jump is an `n` the widget synthesizes; Escape backs out untouched. Chords still bubble over it.
- **`n` and `N`** re-run the committed pattern from the caret; the landing is `Editor::place_at`, so a hit inside a rendered block activates it for free — nothing search-specific in the editor.
- No `?` backward-entry, no regex, no highlight-all, no wrapped-search notice: each waits for demonstrated friction.

## Rejected

- **Searching the raw text with separator fix-ups** — the visible-line table already exists and makes separator hits unrepresentable.
- **Regex** — the notes are prose; substring with smartcase covers the daily ask, and a regex engine is a dependency with its own grammar.
