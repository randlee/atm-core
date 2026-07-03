---
id: AD.7
title: Graft Post-Send Emitter
status: planned
branch: feature/pAD-s7-graft-post-send-emitter
worktree: ../atm-core-worktrees/feature/pAD-s7-graft-post-send-emitter
target: integrate/phase-AD
---

# Sprint AD.7 — Graft Post-Send Emitter

## Goal

- implement the graft-backed post-send emitter

## Hard Dependencies

- `AD.3` complete
- `AD.5` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-T/sprint-T8-atm-graft-crate.md`

## Exact Targets

- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/graft.rs`
- `crates/atm-daemon/src/advisory_runtime.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-graft/src/lib.rs`
- `crates/atm-graft/src/runtime.rs`
- `crates/atm-graft/src/transport.rs`

## Interfaces To Add Or Modify

```rust
pub struct GraftPostSendEmitter { /* owned dependencies */ }

impl PostSendHookEmitter for GraftPostSendEmitter {
    fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError>;
}
```

```rust
fn emit(&self, event: &PostSendHookEvent) -> Result<(), AtmError> {
    self.graft_advisory.deliver_post_send(event)
}
```

- modify the daemon/graft advisory handoff so post-send emission crosses the
  accepted graft advisory/session seam only
- modify graft receive-loop/runtime code so emitted nudges are injected through
  the live advisory path rather than through retired mailbox-context paths
- modify send/ack warning paths so graft-unavailable or advisory-delivery
  failures become sender-visible warnings with structured logs

## Obsolescence Instructions

- any retained graft-side nudge path that bypasses the advisory/session seam
  becomes obsolete in this sprint
- if a transitional unary fetch/drain compatibility path must remain for a
  short period, mark it `Phase AD obsolete: compatibility-only graft nudge
  path`, forbid new production callers, and remove it once the live advisory
  lane proves stable

## Deliverables

- graft-backed recipients receive post-send emission through the approved
  daemon/graft path
- graft emission failures are logged and surfaced as sender-visible warnings

## Required Work

- align graft post-send emission with the simplified AD contract
- use the existing graft host injection seam only as the receiver-side handoff
- keep send success dependent on persistence, not on downstream graft
  consumption

## Error And Warning Contract

The graft emitter must use the shared `AD.3` post-send taxonomy exactly:

- `PostSendGraftUnavailable` / `ATM_POST_SEND_GRAFT_UNAVAILABLE`
  - cause: the recipient graft session or graft host receiver is unavailable
    when emission is attempted
  - sender surface: warning after successful persistence
  - recovery: restore graft receiver availability, then resend only if a
    fresh nudge is still required
- `PostSendAdvisoryDeliveryFailed` /
  `ATM_POST_SEND_ADVISORY_DELIVERY_FAILED`
  - cause: the daemon-to-graft advisory/session handoff failed after message
    persistence
  - sender surface: warning after successful persistence
  - recovery: inspect daemon/graft logs, restore the advisory path, then
    resend only if a fresh nudge is still required

## This Sprint Does Not Close

- local tmux-backed emission
- Claude inbox nudge deletion
- roster drift repair

## Acceptance Criteria

- successful graft emission returns no warning
- unavailable graft recipient or failed graft handoff returns a sender-visible
  warning
- emission failure is logged with enough context to diagnose sender, recipient,
  and graft session scope

## Required Validation

- targeted graft-emitter tests
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `git diff --check`
