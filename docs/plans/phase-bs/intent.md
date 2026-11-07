# Intent - phase-bs: subtask beads (atm-bd-orchestration #937)

Written 2026-11-07 by fenix from Rand's rulings of 2026-11-06 and 2026-11-07. Jev judges against the anchors, not this document.

## What this plan is for

Large plans get split into independent agent tasks with no shared context, and many agents running full lint/test after every rebase pin the CPU and loop test↔rebase before landing. This phase makes child beads the recommended split for concentrated related work: one dev holds the context, runs each child with a background subagent on its own worktree, merges them into one branch, and lands it with one sanity and one QA run.

## What success looks like

- Plans split for short wave graphs: unrelated work in separate sprints, related work as child beads with `fast`/`normal`/`frontier`.
- One dev delivers one tested branch: no subagent PRs; full lint/test runs on a background `fast` agent before the final rebase, not after it.
- One sanity bead and one QA bead per sprint; each dev bead records its commit.
- `bd close` alone enforces children-before-parent.

## What is deliberately out of scope

- Hooks or scripts that enforce close order.
- Explanatory prose; edits to existing text are surgical.
- #938 (ceremony finding screen): its anchors are captured separately.

## Anchors

ANC-001..017 in `anchors.json`, rendered to `anchors.md`.
