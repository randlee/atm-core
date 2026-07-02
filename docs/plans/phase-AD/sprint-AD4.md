# AD.4 Unix-Gating Wrapper Published Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.4
worktree: ../atm-core-worktrees/feature/pAD-s4-unix-gating-wrapper-published-cutover
branch: feature/pAD-s4-unix-gating-wrapper-published-cutover
status: planned
estimated_scope: medium
```

## Goal

Keep `unix-gating` as an ATM-owned subset wrapper while moving its backend to
published portability findings only.

## Scope Summary

This sprint closes only the repo-specific `unix-gating` wrapper cutover.

Production-ready commitment:
- the wrapper must keep the ATM-owned `PORT-004` / `PORT-005` subset contract
- the wrapper must stop depending on the vendored integrated portability path

Required command and filter contract:

```python
command(repo_root) == [
    "sc-lint-portability",
    "analyze",
    "--root",
    str(repo_root),
    "--format",
    "json",
]

RULE_IDS = {"PORT-004", "PORT-005"}
```

## Prerequisites

- `AD.3`

## Out Of Scope

- no `runtime-waits` wrapper cutover yet
- no proc-macro dependency cutover yet
- no vendored crate removal yet
- no CI / release-preflight cutover yet

## Deliverables

- `.just/lint_unix_gating.py` sources findings from the published
  `sc-lint-portability` analyzer
- the wrapper continues to report only `PORT-004` and `PORT-005`
- `.just/tests/test_lint_unix_gating.py` is updated only as needed to preserve
  the ATM subset-wrapper contract

## Acceptance Criteria

- ATM still exposes the `unix-gating` local lint target after retargeting
- non-`PORT-004` / non-`PORT-005` portability findings remain excluded from
  this wrapper
- the wrapper no longer shells through the old vendored portability path
- any stale path-based analyzer invocation left in the repo blocks sprint
  closure

## Required Validation

- `python3 .just/run_lint.py unix-gating`
- `python3 .just/tests/test_lint_unix_gating.py`
- `git diff --check`
