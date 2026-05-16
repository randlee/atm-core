---
id: Y.1
title: ATM Help And UX Improvements
status: planned
branch: feature/pY-s1-atm-help-and-ux
worktree: ../atm-core-worktrees/feature/pY-s1-atm-help-and-ux
target: integrate/phase-Y
---

# Sprint Y.1 — ATM Help And UX Improvements

## Goal

- land the first approved small implementation slice on the Phase `Y` line
- improve `atm help` and adjacent UX text before broader daemon smoke work
- remove or rewrite help/output wording that still implies obsolete mailbox
  truth or mutable shared-inbox behavior

## Hard Dependencies

- `Y.0` must land on `develop`
- `docs/plan-phase-Y.md`
- `docs/phase-Y/inbox-write-path-audit.md`
- `docs/phase-Y/state-machine-coverage-audit.md`
- `GH #83`

## Exact Targets

- `crates/atm/src/main.rs`
- `crates/atm/src/output.rs`
- `docs/atm/`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/phase-Y/sprint-Y1.md`
- `docs/project-plan.md`

## Required Work

- implement the agreed `atm help` improvements and any directly adjacent
  subcommand-help cleanup
- make daemon + SQLite ownership expectations explicit in user-facing help
  where it prevents operator confusion
- remove or rewrite stale help/output text that suggests:
  - shared inbox JSON is ATM’s mutable source of truth
  - normal commands update old inbox messages in place
  - mailbox locks are the correctness boundary for the daemon line
- keep this sprint narrow; do not absorb boundary refactors here
- record any follow-up UX/help fixes that should roll into `Y.2`

## Acceptance Criteria

- `atm help` exists in the form approved for the release line
- command help/output no longer makes stale file-SSOT claims
- any intentionally deferred UX/help items are explicitly listed for `Y.2`

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `git diff --check`
