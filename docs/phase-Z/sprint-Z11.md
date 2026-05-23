---
id: Z.11
title: First Send Recovery Contract And Setup Guidance
status: planned
branch: feature/pZ-s11-first-send-recovery-contract-and-setup-guidance
worktree: ../atm-core-worktrees/feature/pZ-s11-first-send-recovery-contract-and-setup-guidance
target: integrate/phase-Z
---

# Sprint Z.11 — First Send Recovery Contract And Setup Guidance

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.11
worktree: ../atm-core-worktrees/feature/pZ-s11-first-send-recovery-contract-and-setup-guidance
branch: feature/pZ-s11-first-send-recovery-contract-and-setup-guidance
status: planned
estimated_scope: small
```

## Goal

Make the clean-start first-send failure explicit, actionable, and testable
without introducing ambient fallback behavior or a broad roster-import
command.

## Scope Summary

This sprint owns the first-send recovery contract for the clean-start
`Z1-F001` case:

- ATM durable state and daemon startup succeed
- canonical ATM roster is still empty
- first `send` fails because no ATM roster member exists yet

This sprint does not decide automatic roster hydration. It defines the
operator-facing error/recovery contract and the closure proof for that path.

## Governing Requirements

- `REQ-CORE-SEND-003`
- `REQ-CORE-TEAM-001`
- `REQ-CORE-CLAUDE-ROSTER-001`

## Governing ADRs

- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`

## Governing Boundaries

- `RosterStore`
- `InboxExport`
- `RequestDispatcher`

## Prerequisites

- `Z.10` complete

## Hard Dependencies

- `docs/phase-Z/readiness.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`

## Exact Targets

- `crates/atm-core/src/delivery_policy.rs`
- `crates/atm-core/src/send/mod.rs`
- `docs/phase-Z/readiness.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/project-plan.md`

## Delete / Narrow Inventory

- delete the current opaque first-send roster-harness failure wording
- narrow the first-send failure to one typed recovery contract that points
  operators to explicit ATM roster setup
- do not add a `config.json` fallback or implicit roster import

## Non-Goals

- no automatic roster import command
- no watcher / reconcile redesign
- no `teams` / `members` runtime-path cleanup
- no canary/dogfood execution

## Sub-Tasks

1. Replace the bad first-send error contract.
   Development work:
   - replace the current
     `failed to resolve roster-backed delivery harness for <member>@<team>`
     path with exactly this operator-facing recovery contract:
     - `Repair or reload the team roster before retrying delivery.`
     - `Use 'atm teams add-member' for all active team members.`
   - do not append implementation-detail commentary about Claude routing
   Required tests:
   - prove the clean-start first-send failure returns the new recovery text
   Required docs:
   - update `docs/phase-Z/smoke-findings-ledger.md`

2. Prove no ambient fallback was added.
   Development work:
   - keep send-path ownership on canonical ATM roster truth only
   - do not add direct `config.json` routing, roster import, or hidden
     fallback behavior
   Required tests:
   - prove first-send still fails cleanly when ATM roster is empty
   - prove the failure is now actionable rather than opaque
   Required docs:
   - update `docs/phase-Z/readiness.md`

3. Stamp closure records.
   Development work:
   - stamp `Z.11` accepted head and verdict in `docs/phase-Z/readiness.md`
   - add the `Z.11` ledger row to `docs/project-plan.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/project-plan.md`

## Split Recommendation

If the work expands into retained-runtime path cleanup, ambient workspace-config
reads, or public singleton/runtime-factory surface cleanup, stop and move that
scope into `Z.12`, `Z.13`, or `Z.14` instead of widening `Z.11`.

## Acceptance Criteria

- the clean-start first-send failure no longer returns the opaque
  roster-backed harness error text
- the failure returns exactly:
  - `Repair or reload the team roster before retrying delivery.`
  - `Use 'atm teams add-member' for all active team members.`
- no `config.json` roster-truth fallback is introduced
- `docs/phase-Z/readiness.md` records the accepted `Z.11` head and verdict
- `docs/project-plan.md` includes the `Z.11` sprint ledger row

## Non-Closure

- `Z.11` does not add a roster-import command
- `Z.11` does not fix `teams` / `members` retained-runtime path misuse
- `Z.11` does not begin canary/dogfood execution

## Production-Ready Expectation

The first-send empty-roster path must fail with clear, actionable operator
guidance and no hidden fallback behavior.

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `rg -n "failed to resolve roster-backed delivery harness for" crates/atm-core/src`
  - expected: zero user-facing production matches
- `git diff --check`

## Required Document Updates

- `docs/phase-Z/readiness.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/project-plan.md`

## Risks And Watchouts

- do not quietly turn this into automatic roster import
- do not widen this sprint into general command-path cleanup
