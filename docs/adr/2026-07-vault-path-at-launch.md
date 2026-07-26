# Vault path at launch: `NOTE_VAULT` environment variable, default `~/documents/notes`

## Context

`adr/2026-07-dev-test-vault-locations.md` split the fixture vault from the real one and deferred the real path to "configuration, phase 4".
Phase 4 opens a vault for the first time, so the deferral expires here.
The app is single-user with no accounts and no settings UI; the only thing to configure today is one path.

## Decision

The vault path is `$NOTE_VAULT` if set, otherwise `~/documents/notes`.

Resolution is a **pure function of two already-read values**, not a function that reads the environment:

```rust
pub fn resolve_vault_path(note_vault: Option<OsString>, home: Option<OsString>) -> Option<PathBuf>
```

`None` means neither could be determined; the caller reports it and exits rather than guessing a path.
Reading the environment happens once, at the single call site, with `var_os` rather than `var` — a path is bytes on Linux, and `var` turns a perfectly good non-UTF-8 path into `Err(NotUnicode)`, which would silently fall through to the default.

The purity is forced by two constraints meeting: edition 2024 made `std::env::set_var` `unsafe` (and it was always process-global, so env-mutating tests race across the test threads), while `adr/2026-07-coverage-100-percent-lines.md` requires every branch to be covered. A pure resolver is exercised exhaustively with zero `unsafe` and zero test interference.

A missing or non-directory vault is an explicit, reported error state — never silently created, never silently empty. Creating directories on the user's behalf at a possibly-mistyped path is how notes end up in two places.

## Alternatives rejected

- **CLI argument** — has to be retyped or wrapped in a desktop entry at every launch, and adds argument parsing for a single string. Reconsider if multiple vaults ever become a feature.
- **TOML config file** — pulls in `toml` + `serde` and a full not-found/malformed-config error surface to store one path. The env var upgrades to a config file later without changing any caller: the function signature stays the same.
