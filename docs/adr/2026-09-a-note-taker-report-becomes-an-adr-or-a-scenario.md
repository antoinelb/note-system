# A note-taker session reports; a finding becomes an ADR or a scenario, never a note in the repo

## Context

`.claude/agents/note-taker.md` is a persona: someone who keeps a journal and
a zettelkasten, lives in normal mode, and knows nothing about how the app is
built. It never reads `src/`, fixes nothing, and its only sense is the
screenshot. `tests/e2e/session.sh` gives it the app on a headless display
over a throwaway vault (`adr/2026-08-headless-x11-e2e.md` § persona
session). The 2026-09-01 session ran after the CSS-rendering batch landed
and found nine things; the audit asked what the practice is, since the
agent file says how to run one and nothing says when, or what a report is
owed.

## Decision

**A session runs after a batch of interface work lands and before it is
installed.** The gates prove the code does what the tests say; the persona
is the only check that the screen makes sense to someone who did not write
it. It runs on `sonnet`, never on the main session's model — the report is
worth exactly what a fresh pair of eyes sees, not what the author would
rationalise.

**The report is scratch.** It lives in the agent's scratchpad and its final
message, classified Broken / Confusing / Friction with keys pressed,
expectation, outcome, repeat and screenshot. It is never committed: a report
in the repository would be documentation that is neither a decision nor a
test, and would rot the way the neighbouring repo's did.

**A finding that survives review goes one of three ways**: a plan under
`docs/plans/` that fixes every Broken and Confusing item with a scenario
under `tests/e2e/` pinning each (`2026-09-01-note-taker-findings.md` is the
shape), an ADR when the finding turned out to be a decision (loop lines
opening their notes was one), or an explicit out-of-scope line in that plan
naming why it stands (a correct vim behaviour, a settled ADR, the v2
ceiling). Friction is judged, not owed.

## Alternatives rejected

- **A `docs/findings/` directory** — findings without their fix are a todo
  list, and `todo.md` was retired for the same reason
  (`docs/plans/2026-09-02-temporal-panes.md`).
- **Running the persona in the pre-commit hook** — a persona's verdicts are
  judgement, not assertions; the hook runs what fails deterministically.
- **The main session exploring as itself** — it knows what the code intends
  and reads the screen accordingly, which is the failure the persona's
  "never read `src/`" rule exists to prevent.
