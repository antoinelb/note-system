# One idle timer drives both the save and the recompile

## Context

The editor had to choose between explicit save, autosave on leave, and debounced autosave.
Two project constraints narrow it more than taste does:

- `plan.md` § Design principles: **"No hard blocks. All friction mechanisms are soft (visible debt), never save-blockers."**
  Explicit save produces a dirty state, and a dirty state produces a "you have unsaved changes" prompt on navigation — which is a save-blocker in the plainest sense.
- `design/wireframes-v0.md` § Chrome: the whole app has one line of chrome, and the writing sheet has exactly two lines of its own (the note's meta line, and a backlinks footer).
  There is no budget for an unsaved-changes indicator, and no button anywhere for a save affordance to live on.

Typst compilation is also too slow to run per keystroke, so a recompile timer was needed regardless.

## Decision

**One debounce timer, ~500 ms of idle, drives the write and then the recompile.**

```
buffer edited → (500 ms idle) → buffer.save() → cache.render(root, file, text)
                                                        → watcher → index.update_note()
```

No dirty flag, no `Ctrl-S`, no confirm-on-navigate, no unsaved indicator: the file on disk is never more than half a second behind the screen, which is what "plain files are the source of truth" ought to mean in practice.

Consequences worth naming:

- **The app's own writes wake the watcher**, and that is desirable — it is how the index stays live while you type, using machinery phase 3 already shipped and tested.
  The one rule it imposes: a watcher event for the *currently open* note must never reload the buffer from disk, or it would clobber the text mid-edit.
- **The timer only starts on an edit**, so a tick always corresponds to a real change. That is why the buffer needs no dirty flag (`adr/2026-07-buffer-is-path-plus-string.md`).
- **The renderer reads the buffer, not the disk.** `SvgCache::render(root, path, text)` already takes text, so the recompile does not depend on the save having landed first — the two are ordered for the watcher's benefit, not the renderer's.
- **Timing is made testable by `cfg`**, not by a fault module: `QUIET` is 500 ms in a normal build and ~1 ms under `#[cfg(test)]`. `watch.rs` needs the heavier `faults` idiom because it arms per-test; here the same shortened path serves every test.
- **`save` is a plain `fs::write`, not write-to-temp-then-rename.** A crash between truncate and write would truncate a note, but the window is a single small write after an idle pause. Recorded as a known ceiling: if it ever bites, the upgrade is three lines, and a `*.tmp` file inside the vault is already invisible to `scan_vault` (non-`.typ`).

## Alternatives rejected

- **Explicit save (`Ctrl-S`)** — the escape hatch of not-saving is real, but it costs a dirty state, an indicator the design has no room for, and a navigation prompt the plan forbids outright.
- **Autosave on note switch / blur / quit** — fewer writes, but a crash loses the whole session, and "did that save?" becomes a question the writer carries. Autosave whose timing you cannot see is worse than autosave you can stop thinking about.
- **Separate timers for save and recompile** — plausible (save eagerly, recompile lazily), but two timers means two sets of edge cases and the recompile is what makes the pause perceptible anyway.
- **Save on every keystroke** — no timer at all, but one `fs::write` and one watcher event per character.
