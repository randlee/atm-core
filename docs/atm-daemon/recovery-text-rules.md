# ATM Daemon Recovery Text Rules

## Purpose

This document is the persistent recovery-guidance reference started in Phase
V.1 and extended in `V.4`.

It exists so daemon/client runtime recovery rules do not live only in sprint
notes.

## Rule

Daemon/client/runtime failures that a user or operator can act on must include
specific recovery text.

Recovery text must:
- name the failing surface
- describe the next action
- avoid generic “try again later” wording when the user can fix the issue

Recovery text is mandatory for:
- daemon unavailable paths
- socket connect failures
- daemon start failures
- local IPC runtime failures

Stable error code namespace is mandatory for each required recovery category.
`V.1` reserves the namespace and `V.4` must bind each category to concrete
codes in the final daemon/client error surface.

Reserved category codes:
- `ATM_DAEMON_UNAVAILABLE`
- `ATM_SOCKET_CONNECT_FAILED`
- `ATM_DAEMON_START_FAILED`
- `ATM_IPC_RUNTIME_FAILED`

If existing `ErrorCode` enum variants already cover a category, `V.4` may bind
that category to the established variant instead of inventing a second name.
What is forbidden is category drift or ad hoc string-only recovery labels.

## V.4 Category Binding

Phase `V.4` binds the required recovery categories to the existing ATM error
surface rather than inventing a second parallel code family:

| Recovery category | Bound ATM error code / variant | Notes |
|---|---|---|
| `ATM_DAEMON_UNAVAILABLE` | `AtmErrorCode::DaemonUnavailable` via `AtmError::daemon_unavailable(...)` | default daemon/client reachability and runtime coordination failures |
| `ATM_SOCKET_CONNECT_FAILED` | `AtmErrorCode::DaemonUnavailable` via socket/open/bind/connect flavored `AtmError::daemon_unavailable(...)` messages | same code, but the recovery text must explicitly mention the socket/connect surface |
| `ATM_DAEMON_START_FAILED` | `AtmErrorCode::DaemonAutoStartFailed` via `AtmError::daemon_auto_start_failed(...)` | daemon binary missing, spawn failures, publish-timeout auto-start failures |
| `ATM_IPC_RUNTIME_FAILED` | `AtmErrorCode::DaemonUnavailable` or `AtmErrorCode::DaemonLifecycleWedge` depending on whether the failure is one-shot runtime I/O vs lifecycle wedge | local IPC deadlines, dispatch/worker/join-helper failures, lifecycle teardown wedges |

Concrete related variants that remain valid in the final daemon/client error
surface:
- `AtmErrorCode::DaemonLaunchGateRejected`
- `AtmErrorCode::DaemonLifecycleWedge`
- `AtmErrorCode::DaemonServingStateRejected`

`V.4` therefore hardens category-specific `.with_recovery(...)` text on the
concrete variants above instead of renaming the underlying ATM error codes.

Recovery text may be omitted only when:
- the failure is purely internal and no user action exists
- a lower boundary already emitted the exact actionable recovery guidance and
  the higher layer preserves it unchanged

## File Ownership Map

Primary files in scope:
- `crates/atm-daemon-client/src/lib.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/local_ipc_connection.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`

Category ownership:
- daemon unavailable:
  - `crates/atm-daemon-client/src/lib.rs`
  - `crates/atm-daemon/src/composition.rs`
- socket connect failures:
  - `crates/atm-daemon-client/src/lib.rs`
  - `crates/atm-daemon/src/local_ipc_transport.rs`
- daemon start failures:
  - `crates/atm-daemon-client/src/lib.rs`
  - `crates/atm-daemon/src/composition.rs`
- local IPC runtime failures:
  - `crates/atm-daemon/src/local_ipc_transport.rs`
  - `crates/atm-daemon/src/local_ipc_connection.rs`
  - `crates/atm-daemon/src/lifecycle_control.rs`

## Recovery Text Checklist

Each required path should answer:
- what failed
- what the operator should inspect or restart
- whether retry is appropriate
- whether the daemon or host runtime must be restarted

Good examples:
- “Build or install atm-daemon, or set ATM_DAEMON_BIN to the correct executable before retrying.”
- “Restart the daemon; the local IPC listener stopped accepting connections unexpectedly.”
- “Grant write access to the daemon socket parent directory or choose a writable ATM_HOME before retrying.”

Bad examples:
- “operation failed”
- “unexpected error”
- “retry later”

Coverage strategy for `V.4`:
- enumerate the required source files in the sprint plan and this document
- require `.with_recovery(...)` on every daemon-unavailable, socket-connect,
  daemon-start, and local-IPC runtime path in those files unless a lower layer
  already preserves the exact same actionable guidance
- verify `.with_recovery(...)` coverage in QA with checklist review of the
  enumerated paths
- use workspace `cargo clippy -- -D warnings` only for Rust hygiene after the
  recovery-text edits land

## Phase Ownership

- `V.1` establishes this persistent rule set and file ownership map
- `V.1` reserves the stable recovery code namespace for the mandatory
  categories listed above
- `V.4` hardens the concrete rule set, checklist, and enforcement strategy
