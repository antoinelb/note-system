# v0 build order: walking skeleton, read path before write path

## Context

Starting implementation (2026-07-23), the v0 feature list in `plan.md` needed an ordering.
The repo has no code yet, Antoine is learning Rust/Dioxus while building, and the editor is the declared highest-risk component.

## Decision

`roadmap-v0.md` orders v0 in 9 phases along dependencies, not feature bullets:

1. Foundations first (vault on disk → parsing → index), because the editor, backlinks, and open-loops panel all consume the same parse + index layer.
2. A read-only end-to-end skeleton (phase 4) before any editing, to de-risk embedding the typst compiler (`World` trait) early.
3. The split-view editor is built *first* as a stepping stone (phase 5), hybrid block editing upgrades it afterwards (phase 7) — split view is a milestone on the path, not only a fallback.
4. Daily-driving starts at the end of phase 5, before v0 is complete.
5. The logs screen (phase 6) goes directly after the editor, ahead of the hybrid upgrade: it needs only rendering, navigation and create-from-template, and it is what deletes phase 4's scaffolding list. Sitting it behind the riskiest phase would leave the throwaway list in the app accreting features (`2026-07-deux-ecrans-table-et-logs.md`).

## Alternatives rejected

- **Implement the plan's bullets in listed order** — puts the hybrid editor before the link index exists; the riskiest component would block everything behind it.
- **Editor first (hardest-thing-first)** — nothing to render or index against yet, and the steepest possible Rust learning curve on day one.
- **Hybrid editor directly, skipping split view** — loses the cheap intermediate milestone that already replaces Obsidian.
