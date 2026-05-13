# Sprint V.4 — Recovery Context Hardening

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.4"
status: planned
worktree: TBD
branch: TBD
estimated_scope: M
```

## Goal

Make daemon-unavailable and adjacent runtime failure paths consistently carry
actionable recovery guidance so system testing is diagnosable.

Carry-forward reference:
- `RBP-PU-001` disconnected-arm handling and `RBP-PU-002` `join_helper`
  recovery/context findings from the Phase U end-gate are concrete source
  evidence for this sprint.
- `QA-U-002` is the concrete daemon-unavailable recovery/backoff gap this
  sprint must close.

## Scope

- define which daemon/client/runtime errors must carry explicit recovery text
- add a checklist or lint strategy for `.with_recovery()` coverage on the
  required paths
- prioritize:
  - daemon unavailable
  - socket connect failures
  - daemon start failures
  - local IPC runtime failures
- document when recovery text is mandatory versus optional

Files in scope:
- `crates/atm-daemon-client/src/lib.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/local_ipc_connection.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`

Concrete targets:
- daemon unavailable:
  - `crates/atm-daemon-client/src/lib.rs`
    `DaemonSupervisor::ensure_daemon_available*`, `spawn_daemon`, and
    `LaunchGateGuard::*`
  - `crates/atm-daemon/src/composition.rs`
    lifecycle/startup guard and endpoint-guard recovery paths
- socket connect failures:
  - `crates/atm-daemon-client/src/lib.rs`
    daemon auto-start connect/retry and launch-gate timeout paths
  - `crates/atm-daemon/src/local_ipc_transport.rs`
    endpoint bind/listen/publish and accept-loop connection setup failures
- daemon start failures:
  - `crates/atm-daemon-client/src/lib.rs`
    missing daemon binary and failed `Command::spawn()` paths
  - `crates/atm-daemon/src/composition.rs`
    runtime startup/replay/start transition failures before steady serving
- local IPC runtime failures:
  - `crates/atm-daemon/src/local_ipc_transport.rs`
    request/advisory stream deadlines, worker spawn, accept-loop, and dispatch
    failures
  - `crates/atm-daemon/src/local_ipc_connection.rs`
    active-connection drain/shutdown failures
  - `crates/atm-daemon/src/lifecycle_control.rs`
    lifecycle worker install/wake/join failures that break local IPC runtime
    coordination

## Acceptance Criteria

- the required daemon-unavailable error paths are explicitly enumerated
- `.with_recovery()` coverage is checked through a documented checklist, lint,
  or both
- required recovery text is specific and actionable rather than generic
- the resulting rule set is documented for future daemon and client work
- the sprint plan names the concrete daemon-client and daemon source files that
  own the prioritized daemon-unavailable, socket-connect, daemon-start, and
  local-IPC runtime failure paths

## Out Of Scope

- redesigning the full ATM error model
- rewriting unrelated error messages with no daemon/runtime relevance
- process-only sprint-close hygiene
