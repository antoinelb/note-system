# Default templates embedded in the binary, seeded on launch

## Context

A brand-new vault (`adr/2026-08-vault-starts-from-scratch.md`) has an empty — or absent — `templates/` directory, and every creation path reads `templates/<name>.typ` at instantiation time (`template::create`).
The app could not create a single note until the user copied templates in by hand; the `make run` target papered over this for the dev vault by re-copying the fixture templates on every launch, which would also clobber any template edit made outside the repo.

## Decision

The 13 canonical templates (`template.typ` + 12 per-type) are embedded in the binary with `include_str!` of the files under `tests/fixtures/vault/templates/`, and `template::seed(vault)`:

- creates `templates/` and the four category directories (`create_dir_all`),
- writes every missing template via `persist::create_new` — the existence check and the write are one operation, so an existing (user-edited) file is never overwritten and two concurrent seeders cannot clobber each other,
- runs in `main` before the watcher starts, and in `capture::run`, so `app --capture` works against a virgin root too.

A seed failure degrades visibly, never fatally: the app carries it in as `ui::SeedTrouble` root context and reports it on the status surface (`Notice::seed_failed`, `Source::Create`); the headless capture reports it on stderr.
`make run` no longer refreshes the dev vault's templates — the app seeds what is missing, and in-app edits persist.

The fixture files remain the single source of truth: `make check-vault` keeps compile-checking them with the vanilla typst CLI, and the binary cannot drift from them by construction.

## Alternatives rejected

- **A separate canonical templates directory in the repo** — a second copy to keep in sync with the fixtures, for no benefit over `include_str!` pointing at them.
- **Overwrite-on-launch (the old `make run` behaviour, generalized)** — destroys user edits; templates are editable notes (`adr/2026-08-template-editing-in-the-one-editor.md`), so the user's version always wins over the shipped default.
- **Seeding only on an empty vault** — a single deleted or missing template would stay missing; per-file `create_new` restores exactly what is absent and touches nothing else.
