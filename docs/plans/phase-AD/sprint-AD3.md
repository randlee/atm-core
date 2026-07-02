# AD.3 Portability Wrapper Published Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.3
worktree: ../atm-core-worktrees/feature/pAD-s3-portability-wrapper-published-cutover
branch: feature/pAD-s3-portability-wrapper-published-cutover
status: planned
estimated_scope: medium
```

## Goal

Retarget `.just/lint_sc_portability.py` to the published
`sc-lint-portability` binary while keeping the `sc-portability` ATM lint
surface stable and proving portability parity against the old vendored
snapshot.

## Scope Summary

This sprint closes only the full portability-wrapper cutover.

Production-ready commitment:
- `.just/lint_sc_portability.py` must stop sourcing portability findings from
  the integrated vendored `sc-lint-boundary`
- the sprint must prove published portability coverage is at least as strong for
  the ATM repo before consumer-subset wrappers move

Required command contract:

```python
[
    "sc-lint-portability",
    "analyze",
    "--root",
    str(repo_root),
    "--format",
    "json",
]
```

## Prerequisites

- `AD.2`

## Out Of Scope

- no `unix-gating` wrapper cutover yet
- no `runtime-waits` wrapper cutover yet
- no proc-macro registry cutover yet
- no vendored analyzer crate deletion yet
- no CI / release-preflight retarget yet

## Deliverables

- `.just/lint_sc_portability.py` calls the published `sc-lint-portability`
- `.just/tests/test_lint_sc_portability.py` is updated only as needed to
  preserve the existing ATM wrapper contract
- one parity proof is recorded on the same ATM commit as three deterministic
  artifacts:
  - `.triage/phase-AD/ad3-vendored-sc-portability.json`
  - `.triage/phase-AD/ad3-published-sc-portability.json`
  - `.triage/phase-AD/ad3-sc-portability-parity.md`
- the parity summary artifact is produced by the same repo-local comparison
  helper introduced in `AD.2`, or an equivalently deterministic normalization
  command, and records:
  - rule IDs present only in vendored output
  - rule IDs present only in published output
  - any per-rule finding-count drift that requires explanation
- vendored `sc-lint-*` crates remain present after this sprint so parity
  investigation and rollback remain possible

## Acceptance Criteria

- ATM still exposes the `sc-portability` local lint target after retargeting
- the wrapper output shape remains stable enough that the wrapper test still
  expresses the ATM house contract
- the published analyzer reproduces the required ATM portability rule families
- any finding-volume drop, rule disappearance, or incompatible JSON shape
  blocks sprint closure

## Required Validation

- `python3 .just/run_lint.py sc-portability`
- `python3 .just/tests/test_lint_sc_portability.py`
- `cargo run -q -p sc-lint-boundary -- analyze --root . --rule portability --format json > .triage/phase-AD/ad3-vendored-sc-portability.json`
- `sc-lint-portability analyze --root . --format json > .triage/phase-AD/ad3-published-sc-portability.json`
- `python3 .just/compare_sc_lint_findings.py --vendored .triage/phase-AD/ad3-vendored-sc-portability.json --published .triage/phase-AD/ad3-published-sc-portability.json --output .triage/phase-AD/ad3-sc-portability-parity.md`
- `! rg -n 'cargo run -q -p sc-lint-boundary|--rule portability' .just/lint_sc_portability.py .just/tests/test_lint_sc_portability.py -S`
- `git diff --check`
