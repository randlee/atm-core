---
id: Y.9
title: Non-Claude Outbound Boundary Formalization
status: planned
branch: feature/pYb-s9-non-claude-outbound-boundary-formalization
worktree: ../atm-core-worktrees/feature/pYb-s9-non-claude-outbound-boundary-formalization
target: integrate/phase-Yb
---

# Sprint Y.9 — Non-Claude Outbound Boundary Formalization

## Goal

Create a dedicated non-Claude outbound payload boundary so non-Claude delivery
is no longer encoded as hook metadata plus implied behavior.

## Hard Dependencies

- `docs/phase-Yb/sprint-Y8.md` must be complete first

## Governing Requirements

- `docs/phase-Yb/plan-phase-Yb.md`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/phase-Yb/qa-handoff.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Exact Code And Document Targets

- `crates/atm-core/src/boundary.rs`
- `crates/atm-core/src/delivery_execution.rs`
- `crates/atm-daemon/src/non_claude_outbound_runtime.rs`
- `boundaries/atm-core/non-claude-outbound.toml`
- `boundaries/atm-daemon/daemon-non-claude-outbound.toml`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Required Work

1. Define the dedicated non-Claude outbound payload boundary:
   - `atm_core::boundary::NonClaudeOutbound`
   - daemon adapter:
     `atm_daemon::non_claude_outbound_runtime::DaemonNonClaudeOutbound`
2. Make the shared executor contract identical around Claude and non-Claude
   plans.
3. Ensure ack reply and thread update use the same outer executor shape where
   applicable.
4. Remove any remaining dependency on metadata-only post-send-hook payloads as
   delivery proof.
5. Add end-to-end tests for non-Claude outbound payload delivery.

## Acceptance Criteria

- `boundaries/atm-core/non-claude-outbound.toml` and
  `boundaries/atm-daemon/daemon-non-claude-outbound.toml` exist and name the
  allowed caller/adapter relationship
- non-Claude outbound delivery is a first-class payload boundary
- state machines expose the same interface regardless of harness
- the outer execution call graph is shared across harness families
- post-send notification remains notification-only
- named end-to-end tests prove non-Claude payload delivery without relying on
  post-send-hook metadata as delivery evidence

## Required Document Updates

- `docs/architecture.md`
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Required Validation

```bash
cargo fmt --all --check
python3 .just/run_lint.py all
cargo build --workspace
cargo test --workspace
git diff --check
```
