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

If upstream `D.1` is still unreleased when `AD.8` finishes, this sprint
remains the only blocked follow-on sprint under the phase-level checkpoint
policy. `AD.1` through `AD.8` may still be declared functionally complete, but
`AD.9` cannot disappear into an indefinite silent hold.

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

## External Release Checkpoint

If the published `D.1` release is not available by the first ATM release-
planning checkpoint after `AD.8`:

- the branch must record an explicit re-review date against the real upstream
  published state
- and explicit human sign-off must decide whether to keep `AD.9` open or split
  it into a standalone follow-on phase
- indefinite carry-forward with no recorded checkpoint is not allowed

## Out Of Scope

- no speculative adoption of later open-ended Phase `D` follow-on work
- no transitive reachability redesign unless the released `D.1` surface
  requires it explicitly

## Code And Document Targets

- `.just/lint_boundaries.py`
- current `boundaries/**/*.toml` records touched by dependency-policy
  enforcement
- the repo-owned version-pin record for published `sc-lint`
- any ATM-specific tests or docs that remain authoritative for dependency
  policy

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

## Required Work

- pin the first published `sc-lint` release that actually includes `D.1`
- rerun dependency-policy enforcement across the real ATM boundary inventory
- fix any newly exposed boundary TOML drift before claiming closure
- delete or reduce duplicated ATM-local dependency-policy logic only after the
  released analyzer proves equal-or-better coverage
- if upstream release lag blocks execution, record the checkpoint outcome
  explicitly rather than leaving the sprint in an implicit hold

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
- if `D.1` is still unavailable at a release-planning checkpoint, the branch
  records a new checkpoint date or an approved follow-on-phase split instead of
  leaving the sprint in an unbounded implicit stall

## Required Validation

- the released `D.1` dependency-focused boundary command path
- `python3 .just/run_lint.py all`
- any ATM-specific boundary or architecture test surface tied to dependency
  policy
- `git diff --check`
