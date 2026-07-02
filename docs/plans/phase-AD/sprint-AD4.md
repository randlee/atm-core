# AD.4 Unix-Gating Wrapper Published Cutover

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.4
worktree: ../atm-core-worktrees/feature/pAD-s4-unix-gating-wrapper-published-cutover
branch: feature/pAD-s4-unix-gating-wrapper-published-cutover
status: proposed-pending-signoff
estimated_scope: medium
```

## Authorization Gate

This sprint is a proposed planning target only. No implementation branch or
worktree for `AD.4` may open until explicit human sign-off approves `Phase AD`
per [`docs/plans/phase-AD/plan-phase-AD.md`](./plan-phase-AD.md).

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

## Code And Document Targets

- `.just/lint_unix_gating.py`
- `.just/tests/test_lint_unix_gating.py`
- `.triage/phase-AD/ad4-sc-lint-portability-help.txt`
- `.triage/phase-AD/ad4-unix-path-prefixes-review.md`

## Deliverables

- `.just/lint_unix_gating.py` sources findings from the published
  `sc-lint-portability` analyzer
- the wrapper continues to report only `PORT-004` and `PORT-005`
- `.just/tests/test_lint_unix_gating.py` is updated only as needed to preserve
  the ATM subset-wrapper contract
- one repo-owned review artifact
  `.triage/phase-AD/ad4-unix-path-prefixes-review.md` records whether the
  published `sc-lint-portability` surface exposes a direct equivalent for the
  vendored `unix_path_prefixes` knob
- if no direct equivalent exists, the sprint must either carry the behavior
  forward in an ATM-owned wrapper/config override or record an explicitly
  approved removal in that review artifact before sprint closure

## Required Work

- retarget `unix-gating` to published portability findings only
- preserve the `PORT-004` / `PORT-005` subset contract exactly
- inspect the published portability surface for `unix_path_prefixes`
  equivalence
- if direct equivalence is missing, define the repo-owned replacement or
  record approved removal explicitly before closure

## Acceptance Criteria

- ATM still exposes the `unix-gating` local lint target after retargeting
- non-`PORT-004` / non-`PORT-005` portability findings remain excluded from
  this wrapper
- the `unix_path_prefixes` portability-config gap is closed explicitly:
  - direct published equivalence is confirmed
  - or the ATM-owned replacement behavior is named
  - or approved removal is documented
- the wrapper no longer shells through the old vendored portability path
- any stale path-based analyzer invocation left in the repo blocks sprint
  closure

## Required Validation

- `python3 .just/run_lint.py unix-gating`
- `python3 .just/tests/test_lint_unix_gating.py`
- `sc-lint-portability --help > .triage/phase-AD/ad4-sc-lint-portability-help.txt`
- `test -f .triage/phase-AD/ad4-unix-path-prefixes-review.md`
- `rg -n 'direct equivalent|wrapper override|approved removal' .triage/phase-AD/ad4-unix-path-prefixes-review.md -S`
- `! rg -n 'cargo run -q -p sc-lint-boundary|--rule portability' .just/lint_unix_gating.py .just/tests/test_lint_unix_gating.py -S`
- `git diff --check`
