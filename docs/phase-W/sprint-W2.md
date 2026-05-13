---
id: W.2
title: Daemon Client Traceability
status: planned
branch: TBD
worktree: TBD
---

# Sprint W.2 — Daemon-Client Traceability

## Goals

- add per-attempt and exhaustion observability to daemon-client connect and
  auto-start flows
- restore the existing expectation that daemon startup/connect/publish failure
  is visible as a critical issue through both:
  - concise ATM CLI failure output
  - richer `atm doctor` diagnostics and degraded-health evidence
- keep emission/reporting on shared paths rather than creating a daemon-client
  specific reporting stack
- preserve interface parity so the same daemon-availability failure class uses
  the same ATM code/recovery semantics on CLI and `atm-graft` same-host flows

## Hard Dependencies

- depends on `W.1` for the final daemon-side sink-failure rule when new
  traceability events are emitted
- no hard dependency on `W.3` or `W.4`

## Required Work

- add attempt-level tracing for same-host daemon bootstrap/connect paths
- audit current CLI-facing same-host errors against `atm-graft` same-host
  errors and recovery text
- collapse duplicate same-host error/reporting paths when they describe the
  same daemon failure class
- preserve one doctor-facing diagnostic story for same-host failures
- compare every touched same-host failure class against the current `main` CLI
  contract before finalizing any refactor

## Acceptance Criteria

- every path listed in the current path inventory is addressed directly; the
  sprint is not allowed to leave “follow-up discovery” inside daemon-client or
  CLI bootstrap handling
- attempt-level events exist for connect retry, launch-gate contention,
  successful spawn attempt, publish wait, and exhaustion
- final daemon-start/connect failures preserve concise operator-facing ATM
  errors
- `atm doctor` has a documented way to expose the richer daemon-start/connect
  diagnostic trail
- the sprint distinguishes between paths that already satisfy the contract and
  paths that regressed into under-instrumented final failures
- any ATM CLI integration point needed to keep command failures and doctor
  output aligned is treated as required scope, not a deferred follow-up
- the sprint reuses shared observability and doctor reporting paths; only the
  daemon-client event semantics and insertion points are local
- the sprint verifies same-host interface parity between CLI and `atm-graft`
  for daemon-unavailable, auto-start-failed, launch-gate, and advisory-stream
  setup failures
- where CLI and `atm-graft` currently duplicate error-mapping or reporting
  code for the same same-host failure class, the sprint should collapse those
  paths onto one shared implementation
- the sprint identifies the shared ATM error/protocol/doctor functions that
  become the single source of truth for each touched same-host failure class
- req-qa can verify from the sprint doc exactly which same-host surfaces are
  in scope and that CLI-side path tracing is explicitly owned here

## Implementation Notes

Primary insertion points:
- `crates/atm-daemon-client/src/lib.rs`
  - `DaemonSupervisor::ensure_daemon_available(...)`
  - `DaemonSupervisor::ensure_daemon_available_with_timeout(...)`
  - `DaemonSupervisor::ensure_daemon_available_with_lock_path(...)`
  - `DaemonSupervisor::spawn_daemon(...)`
  - `LaunchGateGuard::rejected_error(...)`
  - `LaunchGateGuard::try_acquire_at(...)`
- `crates/atm/src/composition.rs`
  - `LocalIpcClientTransportAdapter::try_connect(...)`
  - `LocalIpcClientTransportAdapter::exchange(...)`
  - `CliComposition::bootstrap(...)`
  - command entrypoints that already surface final ATM errors for
    `send/read/ack/clear/list`
- `crates/atm-graft/src/lib.rs`
  - same-host bootstrap through `DaemonSupervisor`
  - live advisory stream setup and receive-loop startup
- `crates/atm-graft/src/transport.rs`
  - advisory-stream connect / write / flush / timeout setup
- `crates/atm-graft/src/runtime.rs`
  - advisory-stream receive / reconnect / response validation failures

Shared paths that must be reused or consolidated:
- `crates/atm-core/src/error.rs`
  - `AtmError::daemon_unavailable(...)`
  - `AtmError::daemon_auto_start_failed(...)`
- `crates/atm-core/src/protocol.rs`
  - protocol-envelope mapping for same-host daemon failures returned through
    non-CLI consumers
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm/src/commands/doctor.rs`
- `crates/atm/src/output.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`

Current main CLI baseline to preserve:
- `crates/atm-daemon-client/src/lib.rs`
  - final same-host daemon-availability failures already return ATM errors
    through the shared daemon-client bootstrap path
- `crates/atm/src/composition.rs`
  - CLI bootstrap and exchange failures already terminate commands with
    concise ATM errors
- `crates/atm-core/src/error.rs`
  - current ATM error constructors and recovery text remain the baseline for
    daemon-unavailable and auto-start failure classes

Current path inventory:
- `crates/atm-daemon-client/src/lib.rs`
  - `validate_daemon_path(...)`
    - empty-path validation
    - non-UTF-8 path validation
  - `DaemonSupervisor::ensure_daemon_available_with_lock_path(...)`
    - initial `try_connect()` miss
    - retry-loop `try_connect()` miss
    - launch-gate acquisition success
    - launch-gate contention timeout
    - post-spawn publish wait
    - auto-start exhaustion
  - `DaemonSupervisor::spawn_daemon(...)`
    - daemon binary missing
    - `Command::spawn()` failure
  - `LaunchGateGuard::rejected_error(...)`
  - `LaunchGateGuard::try_acquire_at(...)`
    - lock-dir create failure
    - launch-gate open failure
    - launch-gate acquire failure
- `crates/atm/src/composition.rs`
  - `LocalIpcClientTransportAdapter::try_connect(...)`
    - final same-host connect failure returned to CLI
  - `LocalIpcClientTransportAdapter::exchange(...)`
    - write-timeout setup failure
    - read-timeout setup failure
    - request flush failure
    - daemon closed before response
    - response `request_id` mismatch
  - `CliComposition::bootstrap(...)`
    - end-to-end daemon-availability bootstrap failure before command dispatch
- `crates/atm-graft/src/lib.rs`
  - same-host daemon-availability bootstrap before graft runtime start
  - advisory-stream unsupported-path failure
- `crates/atm-graft/src/transport.rs`
  - advisory-stream write-timeout failure
  - advisory-stream flush failure
  - advisory-stream bounded read-timeout failure
- `crates/atm-graft/src/runtime.rs`
  - advisory-stream frame read failure
  - advisory-stream request-id mismatch
  - advisory-stream reconnect/open failure
- CLI command surface that must preserve concise failure output:
  - `atm send`
  - `atm read`
  - `atm ack`
  - `atm clear`
  - `atm list`

Traceability events to add:
- initial same-host connect miss
- repeated connect retry
- launch-gate acquired vs contended
- daemon spawn attempted
- daemon spawn failed
- publish wait continuing
- auto-start timeout exhausted
- launch-gate timeout exhausted while another launch owns the gate

Critical issue classes covered directly by this sprint:
- daemon startup failure
- daemon connect failure
- daemon publish failure
- ATM command failure on the daemon path, especially `atm send` and `atm read`
  when the daemon is unavailable

CLI / doctor split required by this sprint:
- ATM CLI:
  - keep concise returned failure output with stable error code and next action
  - close any drift where the final ATM failure is present but the path-specific
    signal is no longer available
- `atm-graft` host:
  - must receive the same ATM error code and aligned recovery intent for the
    same same-host daemon/connect failure classes
- `atm doctor`:
  - expose the deeper connect / launch / publish trail so operators can
    distinguish “daemon absent,” “daemon spawn failed,” “daemon started but
    never published,” and “launch gate stuck”

Cross-sprint dependency:
- this sprint should reuse the W.1 emit-fallback rule so traceability events
  are not silently lost when the sink is degraded

## Out of Scope

- SQLite writer instrumentation
- peer replay recovery text
- large ATM CLI output redesign beyond what is required for concise failures

## Required Validation

Plan-auditable now:
- explicit ownership of CLI, daemon-client, and `atm-graft` same-host paths
- explicit duplicate-path collapse responsibility
- explicit interface-parity contract

Implementation validation later:
- same ATM code/recovery semantics demonstrated for equivalent same-host
  failure classes across CLI and `atm-graft`
- runtime proof that doctor exposes the deeper daemon-start/connect trail
- proof that duplicate same-host mapping/reporting logic was collapsed onto the
  shared ATM error / doctor paths where the touched failure class existed in
  parallel before
