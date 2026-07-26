# Template import: root-absolute `#import "/templates/template.typ"`, compile with `--root <vault>`

## Context

Every note must compile standalone with the vanilla `typst` CLI and import the shared `template.typ`, which lives in `templates/` alongside the per-type note templates.
Typst sandboxes file access to the project root, which defaults to the note's own directory — so `#import "../template.typ"` fails on a bare `typst compile note.typ` ("would escape the project root"). Verified with typst 0.15.1.

## Decision

- All compilation — `make check-vault`, and later the embedded compiler — passes the vault as the typst root (`--root <vault>` on the CLI, `World` rooted at the vault in phase 4).
- Notes import the template root-absolute: `#import "/templates/template.typ": ...` — identical in every note regardless of directory depth.

## Alternatives rejected

- **Relative `#import "../template.typ"`** — also requires `--root`, but the path varies with nesting depth and reads as escaping the sandbox.
- **Local typst package (`@local/...`)** — notes would compile without `--root`, but the template would live under `~/.local/share/typst/packages`, breaking the vault's self-containment.
