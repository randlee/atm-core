# AD.5 Runtime-Waits Wrapper Published Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.5
worktree: ../atm-core-worktrees/feature/pAD-s5-runtime-waits-wrapper-published-cutover
branch: feature/pAD-s5-runtime-waits-wrapper-published-cutover
status: proposed-pending-signoff
estimated_scope: medium
```

## Authorization Gate

This sprint is a proposed planning target only. No implementation branch or
worktree for `AD.5` may open until explicit human sign-off approves `Phase AD`
per [`docs/plans/phase-AD/plan-phase-AD.md`](./plan-phase-AD.md).

## Goal

Keep `runtime-waits` as an ATM-owned subset wrapper while moving its backend to
the published runtime analyzer.

## Scope Summary

This sprint closes only the repo-specific `runtime-waits` wrapper cutover.

Production-ready commitment:
- the wrapper must keep the ATM-owned runtime-waits subset contract
- the wrapper must stop reading runtime findings out of the old integrated
  vendored boundary analyzer
- the sprint must not assume the published analyzer still emits
  `SCB-RUNTIME-001` / `SCB-RUNTIME-002` unless the retained `sc-lint` rule
  inventory proves it; direct continuity or explicit mapping must be recorded

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

If the published analyzer now emits different rule IDs for the equivalent
runtime-waits findings, the wrapper must use one explicit repo-owned upstream-
to-ATM mapping and still expose only the ATM contract above.

## Prerequisites

- `AD.4`

## Out Of Scope

- no proc-macro registry cutover yet
- no vendored crate removal yet
- no CI / release-preflight cutover yet
- no dependency-policy Phase `D.1` adoption yet

## Code And Document Targets

- `.just/lint_runtime_waits.py`
- `.just/tests/test_lint_runtime_waits.py`
- `.triage/phase-AD/ad5-runtime-waits-rule-map.md`

## Deliverables

- `.just/lint_runtime_waits.py` sources findings from the published
  `sc-lint-runtime` analyzer
- the wrapper continues to report only `SCB-RUNTIME-001` and
  `SCB-RUNTIME-002`
- `.just/tests/test_lint_runtime_waits.py` is updated only as needed to
  preserve the ATM subset-wrapper contract
- one repo-owned rule-map artifact
  `.triage/phase-AD/ad5-runtime-waits-rule-map.md` records either:
  - direct published `SCB-RUNTIME-001` / `SCB-RUNTIME-002` continuity
  - or the explicit published-rule-id to ATM-rule-id mapping used by the
    wrapper

## Required Work

- retarget `runtime-waits` to `sc-lint-runtime`
- preserve the `SCB-RUNTIME-001` / `SCB-RUNTIME-002` subset contract exactly
- inspect the published rule inventory for direct
  `SCB-RUNTIME-001` / `SCB-RUNTIME-002` continuity and record the result in
  the rule-map artifact
- if published rule IDs differ, define one explicit upstream-to-ATM mapping
  rather than leaving the translation implicit in wrapper code
- prove the wrapper no longer reads runtime rule ownership through the old
  integrated boundary analyzer
- block closure if published runtime coverage is weaker than the vendored path

## Acceptance Criteria

- ATM still exposes the `runtime-waits` local lint target after retargeting
- non-runtime findings remain excluded from this wrapper
- any published-rule-id drift is resolved by an explicit rule-map artifact
  rather than implementer-only assumption
- the wrapper no longer shells through the vendored integrated boundary
  analyzer for runtime rule ownership
- any finding-volume drop caused by missing runtime rule coverage blocks sprint
  closure

## Required Validation

- `python3 .just/run_lint.py runtime-waits`
- `python3 .just/tests/test_lint_runtime_waits.py`
- `test -f .triage/phase-AD/ad5-runtime-waits-rule-map.md`
- `rg -n 'direct continuity|published rule id|ATM rule id|SCB-RUNTIME-001|SCB-RUNTIME-002|mapping' .triage/phase-AD/ad5-runtime-waits-rule-map.md -S`
- `! rg -n 'cargo run -q -p sc-lint-boundary|--rule boundaries' .just/lint_runtime_waits.py .just/tests/test_lint_runtime_waits.py -S`
- `git diff --check`
