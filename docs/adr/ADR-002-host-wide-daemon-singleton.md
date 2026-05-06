# ADR-002 — Host-Wide ATM Daemon Singleton

| Field | Value |
|---|---|
| ID | ADR-002 |
| Status | **Accepted** |
| Date | 2026-05-05 |
| Deciders | Rand Lee |
| Relates to | REQ-P-RUNTIME-002, REQ-P-RUNTIME-003, REQ-DAEMON-RUNTIME-001 |
| Supersedes | — |

---

## Context

ATM currently relies on a daemon runtime for local routing, notification,
transport, and cross-host coordination. The current implementation line proved
that a socket-scoped guard is not enough: client-side auto-start, test helper
spawning, and timing-based warmup logic all created paths where a second daemon
could be forked before the loser was rejected.

That is architecturally incorrect. Daemon singleton is requirement `#1` on the
daemon, not an implementation detail and not a testing convenience.

The governing requirement is:

- only one `atm-daemon` process may exist anywhere on the host for the
  supported runtime model

No test, tool, CLI shortcut, alternate socket path, or alternate `ATM_HOME`
value is exempt from this rule.

## Decision Drivers

- daemon singleton is the first daemon requirement
- the current socket-path-local guard is too weak
- tests and tooling must be subordinate to the same production invariant
- the repository needs multiple enforcement layers, not a single fragile check
- implementation and QA must be able to detect violations mechanically

## Options Considered

### Option 1 — Socket-Scoped Singleton Only

Allow singleton enforcement to be derived from the chosen socket path.

**Rejected.** Different socket/home combinations can still produce multiple
daemon processes on one host. This does not satisfy the requirement.

### Option 2 — Host-Wide Singleton With Multiple Guard Layers

Require all daemon launch paths to converge on a host-wide singleton
enforcement model with multiple guard layers.

**Accepted.**

### Option 3 — Allow Test Exceptions

Keep host-wide singleton for production but allow tests to spawn additional
daemon processes.

**Rejected.** This makes the requirement optional in the exact places where the
architecture should be proven. It produces false confidence and incentivizes
designing around the production invariant.

## Decision

ATM adopts a host-wide daemon singleton model.

Required invariant:
- at most one `atm-daemon` process may exist anywhere on the host for the
  supported runtime model

Required guard layers:

1. Pre-spawn launch gate
- client-side auto-start and any other launch initiator must acquire a
  host-wide launch gate before fork/exec
- concurrent CLI processes must not be able to race into parallel daemon spawn

2. Daemon-side startup gate
- daemon startup must refuse to enter serving state when ownership is already
  held
- the daemon must not publish its serving socket before singleton ownership is
  confirmed

3. Static lint / CI gate
- the repository must reject daemon-spawn patterns in ordinary tests and ad hoc
  helper code
- singleton enforcement is therefore protected both at runtime and at review /
  CI time

Required recovery rule:
- stale-owner recovery must preserve the same singleton guarantee as normal
  startup; recovery is not an alternate spawn path

Required failure rule:
- if a daemon already exists, callers must connect to it or fail with a typed
  runtime error
- no code path may silently bypass the daemon by reaching directly into SQLite
  or inbox files

## Consequences

### Positive

- one clear top-level runtime invariant
- client auto-start, daemon runtime, and tests all obey the same ownership
  model
- QA can treat any second-daemon path as a direct blocker

### Negative

- current daemon-spawning test infrastructure becomes invalid
- client auto-start and daemon startup need coordinated redesign
- some existing integration tests must move to in-process transport seams

## Follow-Up Work

- add the dedicated singleton lint gate to `just lint`
- redesign client auto-start around the pre-spawn gate
- redesign daemon startup around the daemon-side serving gate
- delete or quarantine daemon-spawn helpers that violate the invariant
- keep a narrow daemon-runtime suite only for true singleton/runtime behavior
