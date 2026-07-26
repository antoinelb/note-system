# Vault watching: incremental update, fallback to full rebuild

## Context

The index must stay up to date while one writes, without having to rebuild it by hand.
`roadmap-v0.md` planned to "debounce bursts; on anything ambiguous, fall back to a full rebuild".
Two layout constraints weigh on the design:

- `.index/` lives **inside** the vault, so a recursive watch of the root sees SQLite's writes;
- `templates/` lives at the same level as the categories but is not indexed.

## Decision

### Debouncing: `notify-debouncer-full`

A single `:w` produces several inotify events; without debouncing the same note is parsed three times per save.
The quiet window is 200 ms.

The handler passed to the debouncer is a **closure**, not a `Sender` — `impl<F> DebounceEventHandler for F where F: FnMut(DebounceEventResult) + Send + 'static` (`notify-debouncer-full` lib.rs:120-127).
The debouncer already owns its own thread and calls the handler on it, so classification happens inside it and we have no thread of our own.
This is what avoids writing here the event loop without a termination condition that the coding rules forbid: it stays inside the library.

The crate re-exports `notify` (lib.rs:86), so no second dependency is declared.

### Update: incremental, with fallback

A touched note is replaced on its own (`Index::update_note`), a deleted note is removed on its own (`Index::remove_note`).
The cost of this choice is a **second write path** into the index, which could diverge from `rebuild`.
It is neutralized by structure, not by discipline:

- `update_note` deletes then calls the **same** `insert_note` as `rebuild` — the two paths cannot diverge on how a note is stored, only on *which* notes are;
- the `DELETE` on `notes` drops the rows of `tags`, `links` and `anomalies` via `ON DELETE CASCADE`, so no child row can survive a replacement.

Dangling links and typeless notes remain correct without any special handling: they are queries over the full join, computed at read time, never materialized.
Creating the target note of a dangling link therefore makes it stop being dangling without any row of `links` being touched.

`VaultChange::Rescan` triggers a full rebuild and **ends the batch**: once we know events have been lost, the following updates would apply to a state that can no longer be trusted.
Three things produce a `Rescan`: a batch in error, `Event::need_rescan()`, and any event we cannot attribute (`EventKind::Any`, `Other`, `RenameMode::Any`, `RenameMode::Other`).

### Delivery to the consumer: a `Receiver<Vec<VaultChange>>`

The watcher never writes to the index; it reports what changed and the caller decides when to apply it.
The index therefore keeps a single owner, with no `Mutex`, and phase 4 will plug the channel into a Dioxus task.
The tests, for their part, use `recv_timeout`: no wait loop, no `sleep`.

An empty batch is **not** sent.
Without this rule, every SQLite write would wake the caller with zero work to do.

### The filter is not an exclusion list

`note_path` accepts a `.typ` file located directly under a directory that `NoteCategory::from_dir` recognizes.
`.index/`, `templates/`, subdirectories and non-`.typ` files are therefore excluded **because they are not categories**, not because they are named somewhere.
This is exactly the authority that `scan_vault` already uses: there can be no disagreement between what the rebuild indexes and what the watcher watches.

This point is what prevents the feedback loop: a rebuild writes into `.index/`, the watcher sees it, classification returns an empty list, nothing is sent.
The path filter runs **before** the analysis of the event kind, otherwise an `EventKind::Any` on `index.db-wal` would escalate into a `Rescan`, hence into a rebuild, hence into a new write into `.index/`.

### Renames

`notify-debouncer-full` correlates a rename into a single `Modify(Name(RenameMode::Both))` event whose paths are `[origin, destination]` (lib.rs:455-462).
They are therefore read **by position**: the first is removed, the second is parsed.
Each half can fall outside the vault and disappear on its own — moving a note out of the categories is a deletion, moving one in is a creation.

### Coverage: one injection, two dead branches removed

`new_debouncer` can only fail, on a healthy system, through file-descriptor exhaustion.
It does, however, validate its arguments before creating anything: a `tick_rate` greater than the debounce delay is refused before the thread is launched and the watcher created (lib.rs:644-651).
`faults::tick_rate()` exploits this point — `None` outside tests, `QUIET * 2` once armed — on the model of `index::faults`: the injection yields a **value** that the real code uses, never an extra `?`.
Since the `faults` module is `cfg(test)`, only the unit-test compilation can reach this edge; that is why `start` is tested end to end in `src/watch.rs` and not only from `tests/integration/`.

The per-instantiation breakdown also revealed two regions that no test could reach, fixed in **production** rather than worked around:

- `relative.parent()?` in `note_path` — `Path::parent()` only returns `None` for `""` and `/`, and the extension check above already rejects both of those cases. Replaced by `unwrap_or(Path::new(""))`, which `from_dir` refuses anyway.
- the `(None, None)` arm of the rename — the `note_paths.is_empty()` guard guarantees that at least one path of the event is a note, and a correlated rename carries exactly two; the two halves therefore cannot both be outside the vault. The four arms are replaced by `first.into_iter().chain(second).collect()`.

The rule that emerges from this: an unreachable region almost always signals that an earlier line has already settled the question.
The threshold was not asking for one more test, it was pointing at a branch that was not one.

## Rejected alternatives

- **Full rebuild on every burst**: this was the initial recommendation and the roadmap's pre-commitment. Rejected by explicit choice: at the scale of a personal vault the rebuild stays cheap, but it rewrites every row on each debounced keystroke, and the fallback to rebuild is kept anyway for the ambiguous cases, so the incremental path adds no new failure mode — only a fast path.
- **A bare `notify::RecommendedWatcher` with a home-made timer**: a thread, a timer and a shared buffer to write and to cover at 100%, versus a dependency that already does rename correlation by file identifier.
- **Excluding `.index/` by an explicit path test**: two competing definitions of "this is a note", which would diverge the day a category is added.
- **The watcher owns the `Index`**: forces an `Arc<Mutex<Index>>` and makes every UI query go through a lock, to avoid one line of wiring in phase 4.
- **A callback rather than a channel**: the callback runs on a thread we do not control, and the tests would have to extract its state through an `Arc<Mutex<_>>` anyway.

## Consequence

`docs/plan.md` does not need to be modified: "the index is rebuilt by parsing the files; a watcher keeps it up to date" remains accurate, and the rule "on anything ambiguous, full rebuild" is applied as is.

In phase 4, the UI will consume `VaultWatcher::changes` from a Dioxus task and call `watch::apply`.
