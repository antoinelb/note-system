# Vault locations: fixture vault in tests/, real vault outside the repo

## Context

Tests (parsing, index, watcher) need a stable hand-made vault; daily-driving needs a real vault.
Mixing them would either pollute the repo with personal notes or make tests depend on mutable personal data.

## Decision

- **Fixture vault**: `tests/fixtures/vault/`, committed to the repo. Small, hand-made, deliberately includes edge cases (dangling `#l`, missing `#meta`). This is the vault phases 1–3 build and test against.
- **Real vault**: outside the repo, default `~/documents/notes`, path read from configuration once the app opens a vault (phase 4). Never committed.

## Alternatives rejected

- **Fixture only, defer the real path** — the split had to be decided anyway before phase 1 creates directories; deferring only the path buys nothing.
- **Real vault inside the repo** — couples personal notes to the app's git history and makes "clone and run tests" impossible for a clean checkout.
