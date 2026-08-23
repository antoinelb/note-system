# Codex removed, Claude sole implementation agent again

## Context

`adr/2026-08-codex-and-claude-run-in-parallel.md` made Codex the primary implementation agent earlier the same day (2026-08-23), with Claude Code kept supported in parallel through the transition.
Antoine decided to revert to Claude Code only, no specific incident — just settling back on the arrangement from `adr/2026-08-implementation-is-claudes-job.md`.

## Decision

Claude Code is the implementation agent again, exclusively.

- `AGENTS.md` removed — Codex has no repository instruction file here.
- `CLAUDE.md` and `.claude/` remain as they were; nothing there ever named Codex.
- `roadmap-v0.md` § How we work: the Codex/Claude parallel-agent clause replaced with the single-agent phrasing from `adr/2026-08-implementation-is-claudes-job.md`.

## Alternatives rejected

- **Rewrite or delete `2026-08-codex-and-claude-run-in-parallel.md`** — ADRs are the historical record; the parallel-agent decision did happen and stands as written, this ADR only supersedes it going forward.
