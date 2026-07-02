# AD.2 Boundary Wrapper Published Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.2
worktree: ../atm-core-worktrees/feature/pAD-s2-boundary-wrapper-published-cutover
branch: feature/pAD-s2-boundary-wrapper-published-cutover
status: proposed-pending-signoff
estimated_scope: medium
```

## Authorization Gate

This sprint is a proposed planning target only. No implementation branch or
worktree for `AD.2` may open until explicit human sign-off approves `Phase AD`
per [`docs/plans/phase-AD/plan-phase-AD.md`](./plan-phase-AD.md).

## Goal

Retarget `.just/lint_sc_boundary.py` to the published `sc-lint-boundary`
binary while keeping the `sc-boundary` ATM lint surface stable and proving
published parity against the old vendored snapshot.

## Scope Summary

This sprint closes only the full boundary-wrapper cutover.

Production-ready commitment:
- `.just/lint_sc_boundary.py` must stop depending on workspace-built
  `sc-lint-boundary`
- the sprint must prove the published analyzer is not materially weaker on the
  ATM repo before any vendored crate deletion begins

Required command contract:

```python
[
    "sc-lint-boundary",
    "analyze",
    "--root",
    str(repo_root),
    "--format",
    "json",
]
```

The wrapper may resolve the binary path through a helper, but it must not shell
through `cargo run -q -p sc-lint-boundary`.

## Prerequisites

- `AD.1`

## Out Of Scope

- no portability wrapper cutover yet
- no `unix-gating` wrapper cutover yet
- no `runtime-waits` wrapper cutover yet
- no proc-macro registry cutover yet
- no vendored crate deletion yet
- no CI install-path closeout yet

## Code And Document Targets

- `.just/lint_sc_boundary.py`
- `.just/tests/test_lint_sc_boundary.py`
- `.triage/phase-AD/ad2-vendored-sc-boundary.json`
- `.triage/phase-AD/ad2-published-sc-boundary.json`
- `.triage/phase-AD/ad2-sc-boundary-parity.md`
- `.just/compare_sc_lint_findings.py` if no equivalent repo-local comparison
  helper exists yet

## Deliverables

- `.just/lint_sc_boundary.py` calls the published `sc-lint-boundary`
- `.just/tests/test_lint_sc_boundary.py` is updated only as needed to preserve
  the existing ATM wrapper contract
- one parity proof is recorded on the same ATM commit as three deterministic
  artifacts:
  - `.triage/phase-AD/ad2-vendored-sc-boundary.json`
  - `.triage/phase-AD/ad2-published-sc-boundary.json`
  - `.triage/phase-AD/ad2-sc-boundary-parity.md`
- the parity summary artifact is produced by a repo-local comparison helper or
  equivalently deterministic normalization command that records:
  - rule IDs present only in vendored output
  - rule IDs present only in published output
  - any per-rule finding-count drift that requires explanation
- vendored `sc-lint-*` crates remain present after this sprint so parity
  investigation and rollback remain possible

## Required Work

- retarget the boundary wrapper to the published binary only
- preserve the ATM wrapper contract and user-facing lint name
- add or reuse one deterministic parity comparison helper
- produce vendored and published findings on the same ATM commit
- block closure on unexplained rule loss, JSON-shape drift, or finding-count
  drop

## Acceptance Criteria

- ATM still exposes the `sc-boundary` local lint target after retargeting
- the wrapper output shape remains stable enough that the wrapper test still
  expresses the ATM house contract
- the published analyzer reproduces the required ATM boundary rule families
- any unexpected finding-volume drop, rule disappearance, or incompatible JSON
  shape blocks sprint closure

## Required Validation

- `python3 .just/run_lint.py sc-boundary`
- `python3 .just/tests/test_lint_sc_boundary.py`
- `cargo run -q -p sc-lint-boundary -- analyze --root . --format json > .triage/phase-AD/ad2-vendored-sc-boundary.json`
- `sc-lint-boundary analyze --root . --format json > .triage/phase-AD/ad2-published-sc-boundary.json`
- `python3 .just/compare_sc_lint_findings.py --vendored .triage/phase-AD/ad2-vendored-sc-boundary.json --published .triage/phase-AD/ad2-published-sc-boundary.json --output .triage/phase-AD/ad2-sc-boundary-parity.md`
- `! rg -n 'cargo run -q -p sc-lint-boundary' .just/lint_sc_boundary.py .just/tests/test_lint_sc_boundary.py -S`
- `git diff --check`
