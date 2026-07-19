# Ruthless Boundary QA Design Note

`ruthless-boundary-qa` uses the same fenced-JSON findings envelope as the other
review agents so `quality-mgr` can consume it without a custom result path.

What is different is the finding class:

- `boundary_violation` — an active leak or existing boundary break
- `boundary_tightening` — code works, but the boundary can still be narrowed
- `lint_gap` — the repo has a repeated leak pattern with no mechanical guard
- `doc_gap` — boundary docs/TOMLs/ADR guidance are missing or stale

This keeps the agent mechanically compatible with the current QA pipeline while
preserving the user-mandated role: architecture optimization, not only fixed
rule enforcement.

The agent is cadence-limited to QA-1, plan review, and phase review so its
optimization findings do not create endless scoped-recheck churn.
