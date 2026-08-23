# Codex and Claude run in parallel while Codex takes the lead

## Context

The repository gives Claude Code its working instructions through `CLAUDE.md` and its Dioxus 0.7 reference through `.claude/dioxus.md`.
It has no tracked `AGENTS.md`, so Codex does not receive the same project-specific context through its native repository instruction file.
Antoine chose Codex as the primary implementation agent on 2026-08-23 but wants Claude Code to remain usable during the transition.

## Decision

Codex becomes the primary implementation agent, with Claude Code supported in parallel.

- `AGENTS.md` carries the Codex version of the repository instructions.
- `CLAUDE.md` and `.claude/` remain intact so Claude Code keeps its existing path.
- Both agents read the single `.claude/dioxus.md` reference rather than maintaining two copies that can drift.
- This decision changes the development workflow only; it does not choose or change the app's planned v3 AI provider.

This decision replaces the active role assignment in `adr/2026-08-implementation-is-claudes-job.md` without rewriting that historical record.

## Alternatives rejected

- **Rename or remove the Claude files** — that would make the transition exclusive instead of parallel.
- **Duplicate the Dioxus reference under `.codex/`** — two API references would create an avoidable synchronization obligation.
- **Replace the planned v3 `claude` CLI integration here** — the implementation agent and the app's future AI provider are separate decisions.
