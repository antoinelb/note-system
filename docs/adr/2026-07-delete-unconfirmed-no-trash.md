# Delete is immediate: no confirmation, no trash

## Context

Phase 5 adds note deletion to the scaffolding shell.
The usual protections — a confirm dialog, a trash directory, an undo stack — are all UI or storage machinery, and the shell they would live in has a known deletion date (phase 7 replaces it with the logs screen).

## Decision

**Deleting a note removes the file and its index rows immediately; there is no confirmation, no trash, no undo.**

Reasoning:

- The vault is plain files under the user's own version control; recovery is a `git checkout`, not an app feature.
- The consequence that actually matters — links now pointing at nothing — is not silent: it surfaces as visible debt through the existing dangling-links query, per the friction philosophy (no hard blocks, visible debt).
- A confirm dialog would be machinery the headless `VirtualDom` tests must drive, for a widget that dies in two phases.

Revisit when the logs screen replaces the scaffolding shell, where the design can say what deletion should feel like.

## Alternatives rejected

- **Confirm dialog** — protects against a misclick at the cost of every intentional delete; wrong trade for a single-user tool whose data is versioned plain text.
- **Trash directory (`.trash/`)** — a second storage location the index, watcher and `check-vault` would all need to learn to ignore; git already keeps every version.
