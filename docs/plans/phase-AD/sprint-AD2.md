---
id: AD.2
title: Obsolete Config Identity Removal And Doctor Contract Repair
status: planned
branch: feature/pAD-s2-config-identity-removal-and-doctor-repair
worktree: ../atm-core-worktrees/feature/pAD-s2-config-identity-removal-and-doctor-repair
target: integrate/phase-AD
---

# Sprint AD.2 — Obsolete Config Identity Removal And Doctor Contract Repair

## Goal

- remove obsolete config identity usage and close
  `ATM_WARNING_IDENTITY_DRIFT`

## Hard Dependencies

- `AD.1` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/requirements.md`
- `.atm.toml`

## Exact Targets

- repo `.atm.toml`
- `crates/atm-core/src/config/`
- `crates/atm-core/src/doctor/`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/modules/send.md`
- team startup/operator docs touched by identity guidance

## Added Or Modified Surfaces

- modify repo `.atm.toml` so `[atm].identity` is absent from the accepted
  baseline
- tighten doctor wording and operator docs so the only accepted runtime
  identity sources are explicit CLI override and invoking-shell `ATM_IDENTITY`
- keep legacy config parsing fields only when they are still needed to detect
  and report obsolete `[atm].identity`

## Obsolescence Instructions

- `AtmConfig.identity` and any related parse fields may survive only as
  diagnostic-only migration inputs
- if those fields cannot be deleted in this sprint, mark them
  `Phase AD obsolete: diagnostics only`, keep them out of runtime identity
  resolution, and block new production call sites that read them

## Deliverables

- repo config no longer carries `[atm].identity`
- doctor guidance matches the accepted runtime identity model
- no runtime path depends on config identity fallback

## Required Work

- remove obsolete `[atm].identity` from repo-local config
- update doctor/requirements/operator wording so `ATM_IDENTITY` is the accepted
  runtime source
- keep obsolete config parsing only if still required for migration-oriented
  diagnostics

## This Sprint Does Not Close

- caller identity transport semantics
- post-send hook emission
- roster drift repair

## Acceptance Criteria

- `atm doctor --team atm-dev` no longer reports `ATM_WARNING_IDENTITY_DRIFT`
  on the repaired repo baseline
- repo startup docs instruct operators to set `ATM_IDENTITY` in environment,
  not `.atm.toml`
- no accepted runtime path still treats `[atm].identity` as a working fallback

## Required Validation

- targeted doctor/config tests
- `cargo test --workspace`
- `python3 .just/run_lint.py all`
- `git diff --check`
