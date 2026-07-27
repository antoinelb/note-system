# Templates are filled from a closed set of `{{name}}` placeholders, and an unknown one is an error

## Context

All ten per-type templates under `templates/` already use a `{{name}}` convention — `{{id}}`, `{{created}}`, `{{title}}`, and `{{content}}` in `capture.typ` — written in phase 1 and never documented.
Slice 2 is the first code to read them, so the convention has to become a contract.

Two properties make templates unusual input:

- **They are user-editable files inside the vault**, like every other note.
  Editing a template by hand is the intended way to change what a new note looks like, so a typo there is a realistic event, not a hypothetical one.
- **A typo cannot fail loudly on its own.**
  A note containing a literal `{{titel}}` still compiles — typst renders it as text — so it reaches disk, reaches the index, and is only ever caught by a human noticing a strange heading weeks later.

## Decision

**The placeholder set is closed, and a placeholder the caller did not supply is an error that prevents the file from being written.**

```rust
pub enum TemplateError {
    Unreadable(std::io::Error),
    UnknownPlaceholder(String),
}

pub fn fill(template: &str, values: &[(&str, &str)]) -> Result<String, TemplateError>
```

The closed set is enforced by **validating before substituting**, not by an enum of placeholder names:

1. scan the template for every `{{name}}` and reject the first name not present in `values`;
2. only then substitute.

That order is load-bearing, not stylistic.
`{{title}}` is filled from user input, and a note legitimately titled `{{weird}}` would make a post-substitution "are there any `{{` left?" check fail on the user's own text.
Validating the template first means the check only ever looks at the template.

The four names, and who supplies them:

| placeholder | value | used by |
| --- | --- | --- |
| `{{id}}` | the note id (`adr/2026-07-id-scheme-kebab-frozen.md`) | every template |
| `{{created}}` | the creation date, passed in — see below | every template |
| `{{title}}` | what the user typed | the eight permanent types |
| `{{content}}` | captured text, empty in v0 | `capture.typ` only |

Every call supplies all four regardless of which the template mentions, so the table above *is* the closed set and there is no per-template configuration to keep in sync.

Two consequences worth naming:

- **`created` is a parameter, not a clock read.**
  Creation takes the date from its caller, so the UI reads `jiff` once at the edge and the tests are deterministic — the same discipline `render.rs` already enforces with `the_world_has_no_clock`.
- **An unterminated `{{` is treated as plain text, not an error.**
  `{{titel}}` is caught; `{{titel}` is not — it has no closing delimiter, so there is nothing to recognize it by.
  Recorded as a known ceiling: the upgrade is one more error variant, and the typo it misses is rarer than the one it catches.

## Alternatives rejected

- **An enum of placeholder names instead of `&[(&str, &str)]`** — makes the closed set a type rather than a convention.
  Rejected as the same guarantee for more code: the validation pass already rejects anything outside the supplied set, and an enum would add a name → string mapping whose only reader is `fill`.
- **Leave an unfilled placeholder verbatim in the note** — no error path, no test for it, and the note still compiles.
  Rejected because "still compiles" is precisely the problem: the failure is invisible until read, and this is a system whose whole friction design is about making debt visible immediately.
- **Blank out anything unmatched** — the most permissive option, and the one that hides mistakes best: `{{titel}}` silently produces a note with no heading at all.
- **A templating crate (`tinytemplate`, `handlebars`)** — conditionals and loops are features the vault does not want; templates are notes, and a note that branches is no longer editable as a note.
  Substitution of four names is a `str::replace` chain.
