---
id: AD.15
title: Daemon Advisory Runtime Deletion
status: planned
branch: feature/pAD-s15-daemon-advisory-runtime-deletion
worktree: ../atm-core-worktrees/feature/pAD-s15-daemon-advisory-runtime-deletion
target: integrate/phase-AD
---

# Sprint AD.15 — Daemon Advisory Runtime Deletion

## Goal

- delete the daemon-owned graft advisory runtime and return the daemon
  transport/dispatcher path to thin unary request routing plus direct
  post-send emission

## Hard Dependencies

- `AD.14` complete
- `AD.6` complete
- `docs/plans/phase-AD/plan-phase-AD.md`

## Exact Targets

- `crates/atm-daemon/src/advisory_runtime.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/tests_advisory.rs`
- `crates/atm-daemon/src/test_support.rs`
- `crates/atm-daemon/src/daemon_runtime_observability.rs`
- `docs/atm-daemon/boundaries.md`

## Interfaces To Add Or Modify

The accepted post-commit daemon behavior after this sprint is:

```rust
let outcome = persist_message(...)?;
if recipient_has_post_send_hook {
    if let Err(error) = post_send_hook_emitter.emit(&event) {
        log_post_send_failure(&error);
        append_sender_warning(render_post_send_warning(&error));
    }
}
```

The transport receive loop stays unary-only:

```rust
let request = protocol.request_from_frame(frame)?;
let response = dispatcher.dispatch(request)?;
protocol.response_to_frame(response)?;
```

## Paths To Delete

- `crates/atm-daemon/src/advisory_runtime.rs`
- `AdvisoryRuntime` field ownership from `DaemonRequestDispatcher`
- advisory register/unregister/fetch/drain routing in
  `crates/atm-daemon/src/runtime_health.rs`
- advisory-stream special handling in
  `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- advisory-stream support code in `crates/atm-daemon/src/local_ipc_transport.rs`
- advisory-runtime-only tests and helpers that exist solely to support the
  deleted session runtime

## Deliverables

- daemon runtime no longer stores graft session maps or per-session nudge
  queues
- daemon dispatcher no longer owns graft-specific request routing
- local IPC request handling no longer owns receiver-specific streaming logic
- the direct post-send emission path remains intact after the deletion

## This Sprint Does Not Close

- `atm-graft` internal receiver/runtime rewrite
- final smoke/readiness proof

## Acceptance Criteria

- no `AdvisoryRuntime` implementation remains in `atm-daemon`
- daemon request dispatch no longer switches on graft-specific advisory
  request families
- local IPC receive/dispatch code no longer contains special receiver-specific
  stream plumbing
- send/ack post-send warning behavior still flows through the accepted
  `PostSendHookEmitter` contract

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `rg -n "AdvisoryRuntime|dispatch_advisory_stream|RequestEnvelope::Advisory|ResponseEnvelope::Advisory|LocalIpcAdvisoryStreamSink" crates/atm-daemon`
- `git diff --check`
