# The ex line is literal, always global, and speaks its failures

## Context

`:` was on the *not in v2* list (`adr/2026-08-v2-caret-first-order.md`). The
user's nvim sets `gdefault = true`, which is only meaningful to someone who
runs `:s` often enough to have been annoyed by typing `/g`.

## Decision

### The prompt

`:` emits `Act::OpenEx { prefill }`, which opens **the same one-line widget the
`/` prompt uses**, in the same region, wearing the other sigil — AIR LAY-1
spatial constancy, and the sigil is rendered rather than implied (LAY-6).
A visual `:` prefills `'<,'>`, and the selection survives until the line is
submitted (AIR ERR-6). Escape closes it leaving the caret and the selection
exactly as they stood.

### The vocabulary

| Command | Behaviour |
|---|---|
| `:{N}` | go to line N, one-based, landing on its first non-blank |
| `:w` | flush the buffer to disk now rather than at the next debounced pause |
| `:[range]s/old/new/[flags]` | substitute |

- **Literal substrings, never regex.** No regex crate enters for this, and
  reading Typst prose with one is what the never-regex rule already forbids
  elsewhere. The delimiter is whatever non-alphanumeric character follows the
  `s` — vim's own rule, which is what makes `:s#a/b#c#` possible and stops
  `:sort` from parsing its own `o` as a delimiter.
- **Always global.** Every occurrence on every row in range goes, and a
  trailing `g` is accepted and ignored. This is `gdefault`, which is how the
  user's vim is already configured — not stock vim's default, and deliberately
  so.
- **Smartcase, shared with `/`** through `motions::smartcase` and
  `motions::match_len`, so the two cannot drift on what a match is.
- **Ranges**: none (the caret's row), `%`, `'<,'>`, `N`, `N,M`. Reversed and
  overshot row pairs clamp onto a real window. No range arithmetic beyond that
  — no `.`, `$`, `+3`, or marks.
- **One checkpoint, one splice, one `u`** for a whole `:%s` (AIR ACT-2: a
  reversible action executes immediately with undo, never behind a
  confirmation). The bytes between rows — newlines, whole block separators —
  ride along untouched, so a substitution across blocks needs no special case.
  The caret lands at the start of the last row it changed, measured in the
  *new* text.
- **`:q`, `:wq`, `:w!` are not bound.** Leaving a note is Shift+Escape
  (`adr/2026-08-shift-escape-leaves-the-note.md`); binding a second gesture for
  it would be a second answer to a settled question. They fall through to the
  unknown-command notice, which names the real one.

### Failures speak

A *submitted command line* that fails must never fall silent the way an unbound
keystroke does (AIR ERR-2). Every refusal is a `status::Notice` on
`Source::Editor` (`adr/2026-08-status-surface-owns-notices.md`), carrying what
happened and what to type instead (ERR-1):

- `unknown command ":foo" — :w saves, :s/old/new/ substitutes, :12 goes to a line`
- `":s" found no "vieux" in the range — widen it with :%s, or check the case`
- `":s" needs a pattern — spell it :s/old/new/`
- `":42" is past the note's 18 lines — the caret stayed`

**A flagged narrowing of ERR-1:** these carry no copyable diagnostic ID. They
are typos in a one-line prompt with no diagnostic state behind them, this app
is single-user with no telemetry sink, and every other notice in the app
follows the same house shape — an ID here alone would be inconsistent rather
than safer. Recorded rather than smuggled.

## Rejected

- **A regex engine** — a dependency, a second pattern language beside `/`'s,
  and the escaping burden on every literal bracket in a Typst note.
- **Stock `:s` semantics needing `/g`** — the user's own vim does not work that
  way; matching their configured behaviour is the whole reason the ex line is
  here.
- **`:q` / `:wq`** — see above.
- **A second prompt widget** — the `/` prompt's shape is the house answer for a
  one-line command, and two would drift.
