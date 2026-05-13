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
