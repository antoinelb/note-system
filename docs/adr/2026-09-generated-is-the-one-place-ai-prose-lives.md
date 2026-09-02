# `generated/` is the one place AI prose lives, and it has no template

## Context

The vault has four category directories, and `generated/` is the one nothing
writes to yet. `template::seed` creates it at every launch beside the other
three, `NoteCategory::from_dir` and `NoteType::Generated` name it, the logs
rail lists whatever it holds by creation date beside the captures
(`Index::captured_on`), and a table card wears the `generated` label and its
own bar. `create::TYPES` does not offer it, and `templates/` ships no
`generated.typ`. The 2026-09-02 audit found the directory decided everywhere
but recorded nowhere.

## Decision

**`generated/` is the sole exception to "AI never writes prose in note
files"** (CLAUDE.md § Load-bearing invariants): a note there is machine
output — v3's digests and summaries, written by the `claude` CLI pipeline
under the explicit `generated` type — and the directory makes "what did the
AI write" one `ls`. Being a plain `.typ` file it stays the user's to edit or
delete; the app never regenerates over an edit.

**No template, and no Ctrl+N row.** A template exists so a person can start
a note from a shape; nobody starts a generated note. Its shape is the
writer's decision and arrives with v3
(`adr/2026-08-v3-mcp-first-order.md`), not before.

**The rail treats it like a capture**: listed on the day it was created,
never a debt. What it summarises may be a loop; the summary itself is not.

## Alternatives rejected

- **A `generated` type under `permanent/`** — type is a `#meta` field, so
  the index could find them; but the invariant this directory guards is
  about authorship, and a directory is the one thing a shell can check
  without the index.
- **A placeholder template now** — a shape guessed before the writer exists
  would be rewritten by v3 and is one more file the vault skeleton seeds.
