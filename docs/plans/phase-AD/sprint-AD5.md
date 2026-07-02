# AD.5 Runtime-Waits Wrapper Published Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.5
worktree: ../atm-core-worktrees/feature/pAD-s5-runtime-waits-wrapper-published-cutover
branch: feature/pAD-s5-runtime-waits-wrapper-published-cutover
status: planned
estimated_scope: medium
```

## Goal

Keep `runtime-waits` as an ATM-owned subset wrapper while moving its backend to
the published runtime analyzer.

## Scope Summary

This sprint closes only the repo-specific `runtime-waits` wrapper cutover.

Production-ready commitment:
- the wrapper must keep the ATM-owned runtime-waits subset contract
- the wrapper must stop reading runtime findings out of the old integrated
  vendored boundary analyzer

Required command and filter contract:

```python
command(repo_root) == [
    "sc-lint-runtime",
    "analyze",
    "--root",
    str(repo_root),
    "--format",
    "json",
]

RULE_IDS = {"SCB-RUNTIME-001", "SCB-RUNTIME-002"}
```

## Prerequisites

- `AD.4`

## Out Of Scope

- no proc-macro registry cutover yet
- no vendored crate removal yet
- no CI / release-preflight cutover yet
- no dependency-policy Phase `D.1` adoption yet

## Deliverables

- `.just/lint_runtime_waits.py` sources findings from the published
  `sc-lint-runtime` analyzer
- the wrapper continues to report only `SCB-RUNTIME-001` and
  `SCB-RUNTIME-002`
- `.just/tests/test_lint_runtime_waits.py` is updated only as needed to
  preserve the ATM subset-wrapper contract

## Acceptance Criteria

- ATM still exposes the `runtime-waits` local lint target after retargeting
- non-runtime findings remain excluded from this wrapper
- the wrapper no longer shells through the vendored integrated boundary
  analyzer for runtime rule ownership
- any finding-volume drop caused by missing runtime rule coverage blocks sprint
  closure

## Required Validation

- `python3 .just/run_lint.py runtime-waits`
- `python3 .just/tests/test_lint_runtime_waits.py`
- `! rg -n 'cargo run -q -p sc-lint-boundary|--rule boundaries' .just/lint_runtime_waits.py .just/tests/test_lint_runtime_waits.py -S`
- `git diff --check`
