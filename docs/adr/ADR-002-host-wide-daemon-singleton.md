# ADR-002 — Host-Wide ATM Daemon Singleton

| Field | Value |
|---|---|
| ID | ADR-002 |
| Status | **Superseded by ADR-026** |
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

Cross-reference note:
- `REQ-P-RUNTIME-002` and `REQ-P-RUNTIME-003` are the authoritative product
  requirement anchors after the branch merges complete

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
- the launch gate is the stable `~/.atm/daemon/launch.lock` file acquired with
  `fs4::FileExt::try_lock_exclusive`
- failed launch-gate acquisition returns one typed `already_owned` admission
  outcome rather than a blocking wait loop

2. Daemon-side startup gate
- daemon startup must refuse to enter serving state when ownership is already
  held
- the daemon must not publish its serving socket before singleton ownership is
  confirmed
- the startup gate is the stable `~/.atm/daemon/owner.lock` file acquired with
  `fs4::FileExt::try_lock_exclusive`
- launch-to-owner handoff is:
  1. launcher acquires and holds `launch.lock` before fork/exec
  2. daemon acquires `owner.lock` before publishing a local endpoint or
     entering serving state
  3. launcher releases `launch.lock` only after the daemon confirms serving
     state
  4. if the daemon cannot acquire `owner.lock`, startup fails closed and the
     launcher releases `launch.lock`
- Windows follows the same typed handoff contract even though the underlying
  lock implementation uses Windows file-lock primitives instead of Unix
  advisory locks

3. Static lint / CI gate
- the repository must reject daemon-spawn patterns in ordinary tests and ad hoc
  helper code
- singleton enforcement is therefore protected both at runtime and at review /
  CI time

Required recovery rule:
- stale-owner recovery must preserve the same singleton guarantee as normal
  startup; recovery is not an alternate spawn path

Required lock-shape rule:
- singleton ownership uses stable permanent lock-file paths under
  `~/.atm/daemon/`
- `launch.lock` and `owner.lock` are canonical lock files, not ephemeral
  sentinel paths deleted to signal handoff
- the cross-platform lock foundation is one whole-file exclusive-lock contract
  on those stable file paths
- owner-visible metadata is the documented `pid[:token]` record stored in the
  held lock-file contents
- the metadata token is:
  - a daemon-generated opaque ASCII identifier
  - unique per daemon start attempt
  - persisted alongside the pid so stale-owner recovery can distinguish a new
    daemon from a recycled pid
  - generated before serving state publication and rewritten only by the
    current lock holder
- stale-owner recovery must validate both pid liveness and token continuity
  before replacing owner metadata under a held `owner.lock`
- release clears or invalidates the owner metadata before the exclusive lock is
  released
- supported singleton deployment assumes `~/.atm/daemon/` is on a local
  filesystem with working host-local advisory lock semantics; NFS or other
  network-mounted roots are an accepted limitation and are not a supported
  production singleton configuration

Required failure rule:
- if a daemon already exists, callers must connect to it or fail with a typed
  runtime error
- no code path may silently bypass the daemon by reaching directly into SQLite
  or inbox files

Required singleton error inventory:
- singleton launch gate rejection
- daemon serving-state rejection after ownership is already held
- stale-owner recovery failure
- daemon auto-start failure after the documented retry budget
- fake HTTP application-client injection failure when a test requests an
  invalid runtime seam

Error-model note:
- these failure modes require stable ATM-owned error codes with structured
  context and recovery guidance following the direction in
  `.claude/skills/rust-best-practices/patterns/error-context-recovery-plan.md`
- required inventory for the Phase R/R.10 line:

| Code | Cause | Recovery steps |
| --- | --- | --- |
| `ATM_DAEMON_LAUNCH_GATE_REJECTED` | the client-side pre-spawn singleton gate detected an already-owned daemon runtime and refused to fork a second process | connect to the existing daemon if it is healthy; otherwise resolve stale ownership through the documented recovery path instead of forcing a second spawn |
| `ATM_DAEMON_SERVING_STATE_REJECTED` | the daemon-side serving gate determined ownership was already held before the process could enter serving state | stop launching duplicate daemons; inspect the existing daemon process and its runtime health before retrying |
| `ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED` | startup could not safely recover a stale owner record while preserving singleton guarantees | inspect the recorded owner, confirm no live daemon remains, repair ownership metadata, then retry startup |
| `ATM_DAEMON_AUTO_START_FAILED` | the CLI exhausted the documented auto-start retry budget without reaching a healthy daemon serving state | inspect daemon stderr/logs, fix the startup fault, and retry only after the daemon runtime can pass the documented readiness checks |
| `ATM_TEST_FAKE_TRANSPORT_INJECTION_FAILED` | a test requested an invalid or incomplete in-process HTTP application seam instead of an approved Tier 1 or Tier 2 double | configure a valid fake HTTP application client or in-process HTTP adapter before rerunning the test |

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
