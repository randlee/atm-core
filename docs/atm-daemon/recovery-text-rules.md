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

## Phase Ownership

- `V.1` establishes this persistent rule set and file ownership map
- `V.4` hardens the concrete rule set, checklist, and enforcement strategy
