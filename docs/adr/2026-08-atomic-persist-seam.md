# Atomic persistence: one seam under every byte that reaches disk

## Context

The AIR architecture review (C2) found the same blind `fs::write` under all three writers — the editor's save, the positions file, note creation — and creation used a `path.exists()` pre-check a second writer could race past (TOCTOU), while `index.rs` already stated the correct principle for its own file.
The debounced-autosave ADR had recorded the crash-truncation window as a known ceiling with a named upgrade path.

## Decision

Taken with the user (2026-08-13):

- **A `persist` module owns every write**: `write_atomic` (write a `.tmp` sibling, `sync_all`, rename over the target) and `create_new` (existence check and creation as one `O_EXCL` operation).
- **The fsync is load-bearing, not optional**: without it, a crash shortly after the rename can still surface an empty file on ext4 — the very truncation the seam exists to close.
- `write_atomic` returns the written file's mtime, read from the temp file **before** the rename (which preserves it), so the external-edit guard's stamp can never race a later writer.
- The temp is `{target}.tmp` in the same directory: the rename never crosses a filesystem, and the non-`.typ` extension keeps it invisible to the watcher and the vault scan.
- A failed write sweeps its temp best-effort; a temp orphaned by a crash is overwritten by the next save and visible to nothing.
- Save-failure tests now lock the **directory**, not the file: an atomic write never opens the target, so only the directory can refuse it.

## Rejected

- **Promoting the `tempfile` crate to a production dependency** — it is dev-only today, and the whole seam is ~30 lines of stdlib; a new dependency for that fails the ladder.
- **Upgrading each writer inline, no shared module** — three copies of the same subtle sequence (sync order, stamp-before-rename) is exactly the leaked-interface shape the review flagged.
- **Skipping the fsync for speed** — notes are small and the write is debounced; the cost is microscopic against the failure it closes.
