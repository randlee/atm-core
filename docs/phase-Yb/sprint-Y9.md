---
id: Y.9
title: Non-Claude Outbound Boundary Formalization
status: complete
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
- `docs/phase-Yb/removal-ledger.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/boundaries.md`

## Required Work

1. Define the dedicated non-Claude outbound payload boundary:
   - `atm_core::boundary::NonClaudeOutbound`
   - daemon adapter:
     `atm_daemon::non_claude_outbound_runtime::DaemonNonClaudeOutbound`
   - executor type:
     `atm_core::delivery_execution::NonClaudeOutboundDeliveryWriter`
2. Make the shared executor contract identical around Claude and non-Claude
   plans.
3. Ensure ack reply and thread update use the same outer executor shape where
   applicable.
4. Remove any remaining dependency on metadata-only post-send-hook payloads as
   delivery proof.
5. Delete the retained non-Claude fallback surfaces only after the dedicated
   boundary exists.
6. Add end-to-end tests for non-Claude outbound payload delivery.

## Acceptance Criteria

- `boundaries/atm-core/non-claude-outbound.toml` and
  `boundaries/atm-daemon/daemon-non-claude-outbound.toml` exist and name the
  allowed caller/adapter relationship
- `atm_core::delivery_execution::NonClaudeOutboundDeliveryWriter` exists and
  is the only approved non-Claude payload executor type
- non-Claude outbound delivery is a first-class payload boundary
- state machines expose the same interface regardless of harness
- the outer execution call graph is shared across harness families
- post-send notification remains notification-only
- the sprint closes ledger rows:
  - `YB-RM-012` through `YB-RM-016`
  - `YB-RM-019`
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

## Validation Record

- branch closeout validated with:
  - `cargo fmt --all --check`
  - `python3 .just/run_lint.py all`
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `git diff --check`
- follow-up hardening notes:
  - `RSH-Y9-001` is waived because `NonClaudeOutbound::deliver_payloads(...)`
    runs under the daemon's thread-per-connection (`std::thread`) IPC model,
    so `spawn_blocking` is not applicable there
  - `RSH-Y9-002` is a no-op on this branch because
    `atm_core::protocol::MAX_DAEMON_FRAME_BYTES` already bounds the request
    upstream before it reaches
    `crates/atm-daemon/src/non_claude_outbound_runtime.rs`; the branch adds
    the explicit code comment instead of a second size guard
  - `RSH-001` is documented in-code only: blocking filesystem I/O here is
    accepted because the daemon's thread-per-connection model is already
    capped by `MAX_CONCURRENT_CONNECTIONS` (64)
