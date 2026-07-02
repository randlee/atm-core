# AD.9 Dependency-Policy Ownership Cutover On Released Phase D.1

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.9
worktree: ../atm-core-worktrees/feature/pAD-s9-dependency-policy-ownership-cutover
branch: feature/pAD-s9-dependency-policy-ownership-cutover
status: planned
estimated_scope: medium
```

## Goal

Adopt the first released `sc-lint` version that includes Phase `D.1`
dependency-policy enforcement and move dependency-policy ownership onto the
released analyzer wherever it provides equal-or-better coverage.

## Scope Summary

This sprint closes only the dependency-policy ownership cutover.

Production-ready commitment:
- ATM boundary inventory must be proven against the released `D.1`
  dependency-policy rule family, not only against ATM-local parsing or review
  checks
- duplicate ATM-local dependency-policy checks must be deleted or reduced once
  released `sc-lint` proves equivalent or stronger coverage

Dependency-policy records that must be validated against released `D.1`:

```toml
[dependencies]
allowed_dependents = ["..."]
allowed_dependencies = ["..."]
forbidden_edges = ["left-package -> right-package"]
```

## Prerequisites

- `AD.8`
- a published `sc-lint` release exists that contains Phase `D.1`

## Out Of Scope

- no speculative adoption of later open-ended Phase `D` follow-on work
- no transitive reachability redesign unless the released `D.1` surface
  requires it explicitly

## Deliverables

- ATM pins the first released `sc-lint` version that includes direct
  dependency-policy enforcement from Phase `D.1`
- ATM reruns its boundary inventory against the released dependency-policy rule
  family
- any ATM boundary inventory drift exposed by real `D.1` enforcement is fixed
  in ATM before phase closeout
- duplicated ATM-local dependency-policy checks in `.just/lint_boundaries.py`
  are deleted or reduced to ATM-only governance checks where released
  `sc-lint` is now authoritative

## Paths To Delete

- dependency-policy enforcement code in `.just/lint_boundaries.py` that the
  released `sc-lint` `D.1` surface fully supersedes

## Acceptance Criteria

- Phase `AD` does not close until the released `D.1` dependency-policy rule
  family is active and green on ATM
- any ATM boundary TOML edits required by real `D.1` enforcement land in this
  sprint rather than being deferred
- any residual dependency-policy logic left in `.just/lint_boundaries.py` is
  explicitly justified as ATM-only governance rather than silent duplication

## Required Validation

- the released `D.1` dependency-focused boundary command path
- `python3 .just/run_lint.py all`
- any ATM-specific boundary or architecture test surface tied to dependency
  policy
- `git diff --check`
