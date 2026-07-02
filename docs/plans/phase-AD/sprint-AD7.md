# AD.7 Vendored Crate Removal

```yaml
plan_type: sprint_plan
phase: AD
sprint: AD.7
worktree: ../atm-core-worktrees/feature/pAD-s7-vendored-crate-removal
branch: feature/pAD-s7-vendored-crate-removal
status: planned
estimated_scope: medium
```

## Goal

Delete the vendored `sc-lint-*` workspace members after wrapper and
proc-macro parity are already proven.

## Scope Summary

This sprint closes only the duplicate vendored-crate removal line.

Production-ready commitment:
- the ATM repo must stop carrying the old duplicated `sc-lint` implementation
- wrapper, proc-macro, and install-path behavior must already be proven before
  deletion starts

## Prerequisites

- `AD.6`

## Out Of Scope

- no CI / release-preflight cutover yet
- no dependency-policy Phase `D.1` adoption yet

## Code And Document Targets

- root `Cargo.toml`
- `Cargo.lock`
- `crates/sc-lint-directives/`
- `crates/sc-lint-attributes/`
- `crates/sc-lint-boundary/`
- any wrapper or helper path that still assumes those directories exist

## Deliverables

- ATM removes the vendored workspace members:
  - `crates/sc-lint-directives`
  - `crates/sc-lint-attributes`
  - `crates/sc-lint-boundary`
- no ATM workspace path dependency points at vendored `sc-lint-*`
- no lint wrapper still shells through a workspace-built vendored `sc-lint`
  binary

## Required Work

- remove the vendored workspace members from the workspace definition
- delete the vendored crate directories and update lockfile state
- remove any residual path assumptions in wrappers, tests, or helper scripts
- prove the previously cut-over wrappers still run after deletion

## Paths To Delete

- `crates/sc-lint-directives/`
- `crates/sc-lint-attributes/`
- `crates/sc-lint-boundary/`

## Acceptance Criteria

- the vendored `sc-lint-*` crates are absent from the ATM workspace
- no `Cargo.toml` still uses a path dependency into the deleted vendored crates
- no wrapper/backend path in ATM still depends on those deleted crates being
  present

## Required Validation

- `cargo build --workspace`
- `python3 .just/run_lint.py sc-boundary`
- `python3 .just/run_lint.py sc-portability`
- `python3 .just/run_lint.py unix-gating`
- `python3 .just/run_lint.py runtime-waits`
- `rg -n 'path = \"\\.\\./sc-lint-(attributes|directives|boundary)\"|crates/sc-lint-(attributes|directives|boundary)' Cargo.toml crates/*/Cargo.toml -S`
- `git diff --check`
