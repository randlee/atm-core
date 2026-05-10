# Sprint S.13 IPC/Socket Runtime Hardening Plan

**Branch**: feature/pS-s13-ipc-transport-plan  
**Base**: integrate/phase-S @ 77badd5  
**PR target**: integrate/phase-S  
**Status**: Planning

## Goal

Define the next daemon transport hardening step after S.12. The local IPC
server path needs a simpler correctness story: one accept loop, one receive
loop per accepted connection, one explicit shutdown signal, and fatal-path
cleanup that cannot wedge the daemon in a half-dead state. Peer transport
remains a separate concern because the current Phase S implementation is an
outbound per-message TCP client with retry/replay, not a second inbound
listener.

This plan incorporates the independent Opus architect guidance already
accepted in tasking:
- `ShutdownBeacon`
- `SocketEndpointGuard`
- typed daemon exit codes
- explicit runtime SLOs

## Current Failure Shape

The current local IPC serving path in
`crates/atm-daemon/src/local_ipc_transport.rs` splits shutdown and transport
control across:
- a lifecycle waiter thread
- an accept thread
- an event-channel-driven serve loop
- per-connection request workers

That shape works on the happy path, but it is harder to prove on fatal paths.
In particular, an accept failure can break the serve loop while a scoped
lifecycle waiter is still blocked in `wait_for_state_change()`. Because the
runtime uses `thread::scope`, that can turn a transport fault into a shutdown
wedge instead of a clean bounded exit.

The peer transport in `crates/atm-daemon/src/peer_transport.rs` is different:
- one TCP stream per delivery attempt
- one framed request
- one framed response
- retry/replay classification on failure

S.13 therefore treats local IPC and peer sockets as related but separate
runtime loops rather than forcing them into one premature abstraction.

## Scope And Non-Goals

In scope:
- redesign of same-host local IPC listener/connection control flow
- explicit shutdown/cancellation contract for local IPC fatal paths
- endpoint cleanup and owner-release ordering
- typed process-exit taxonomy for daemon supervisors
- peer-transport concerns that need to be tracked separately

Out of scope:
- replacing the current peer transport with persistent sessions
- `atm-core` business logic changes
- SQLite/rusqlite concurrency redesign
- daemon-spawning test exceptions

## Design Areas

### 1. Single Receive-Loop Per IPC Connection

### Target flow

1. `RuntimeComposition::start()` installs lifecycle control, acquires
   host-wide ownership, binds local IPC, and constructs a `SocketEndpointGuard`
   before entering serving state.
2. The serving thread owns the accept loop directly. It is no longer an
   event-bus consumer waiting on a second accept thread.
3. Each accepted stream spawns one connection runtime that owns:
   - the socket handle
   - a clone of `ShutdownBeacon`
   - the bounded request/response deadline budget
   - one tracked dispatch registration
4. The connection runtime executes one receive loop:
   - `read_frame`
   - decode and validate request
   - dispatch request asynchronously into the daemon service boundary
   - wait on a bounded response channel
   - `write_frame`
   - continue until EOF, shutdown, or transport error
5. EOF and request-local socket failures close only that connection.
6. Accept-loop fatal failures request daemon shutdown through
   `ShutdownBeacon`, wake the listener if needed, drain tracked work, clean up
   the endpoint, and return a typed runtime exit.

### Thread ownership

- `RuntimeComposition` thread:
  - owns the accept loop and the top-level fatal result
- `LifecycleControl` waiter:
  - one helper thread that only observes lifecycle state and requests shutdown
    or reload through `ShutdownBeacon`
- `ConnectionRuntime` thread per accepted stream:
  - owns socket I/O for that connection only
- `RequestDispatcher` worker:
  - owns business logic only and never owns socket lifetime or process-liveness
    decisions

### Cancellation contract

- `ShutdownBeacon` is the only accepted process-wide transport shutdown signal.
- The accept loop, lifecycle waiter, and every connection runtime must observe
  the same beacon instance.
- Connection runtimes may finish an in-flight response if they can do so within
  the bounded shutdown budget; otherwise they must fail closed with a typed
  transport/runtime error and release the socket.
- Detached, untracked request execution remains forbidden under
  `REQ-DAEMON-TRANSPORT-004`.

### Why this shape

This keeps transport proof obligations local:
- listener correctness lives in one loop
- per-connection correctness lives in one loop
- dispatch correctness stays behind one async boundary
- shutdown correctness is driven by one shared signal

That is materially easier to reason about than the current
accept-thread-plus-event-channel structure.

### 2. `ShutdownBeacon`

`ShutdownBeacon` is a crate-private runtime control primitive, not a new public
extension surface.

### Required behavior

- idempotent `request_shutdown(reason)`
- idempotent `request_reload(reason)`
- cheap `is_shutdown_requested()`
- wakeable wait primitive for loops that otherwise block indefinitely
- stable typed reason for observability and process-exit mapping

### Recommended shape

- `Arc<ShutdownBeacon>`
- internal state:
  - atomic phase flag
  - generation counter
  - condvar-backed waiter or equivalent wake primitive
- observers:
  - accept loop checks before and after `accept()`
  - lifecycle waiter requests terminate/reload through the beacon instead of
    pushing `ServeEvent`s
  - connection runtimes poll before read/write and before waiting on a
    dispatch result

### Required wake path

When shutdown is requested:
- set beacon state first
- wake the listener second
- let the accept loop observe the shutdown state after unblock

This ordering ensures the wakeup cannot be mistaken for a fresh external
connection and keeps the fatal path deterministic.

### 3. `SocketEndpointGuard`

`SocketEndpointGuard` owns the published same-host endpoint from the moment the
listener is ready until the daemon has fully stopped serving.

### Responsibilities

- remember the logical endpoint identity and concrete OS binding
- own endpoint cleanup on drop
- remove Unix socket-path artifacts when required
- guarantee endpoint unpublication before daemon ownership release

### Drop contract

1. serving state stops accepting new connections
2. connection drain begins
3. `SocketEndpointGuard` drops and unpublishes the endpoint
4. host ownership is released afterward

This ordering is required by ADR-002. The host must never re-advertise
"daemon available" by releasing `owner.lock` while the old endpoint is still
reachable or stale.

### Launch/owner ordering

S.13 keeps the accepted ADR-002 startup contract:
1. launcher holds `launch.lock`
2. daemon acquires `owner.lock`
3. daemon binds and publishes the endpoint
4. launcher releases `launch.lock` only after serving readiness is confirmed

The S.13 addition is the symmetric shutdown rule:
- endpoint unpublished before `owner.lock` release

### 4. Typed Exit Codes And Supervisor Contract

S.13 introduces a daemon-private exit taxonomy so fatal transport/runtime paths
become machine-actionable instead of collapsing into one generic error exit.

| Exit | Meaning | Typical causes | Supervisor contract |
|---|---|---|---|
| `0` | clean stop | operator-requested shutdown, orderly terminate path | no restart unless explicitly configured |
| `1` | invariant/internal bug | panic boundary, impossible-state violation, poisoned control primitive | treat as bug; restart allowed but alert immediately |
| `64` | configuration/bootstrap contract failure | invalid runtime home, invalid endpoint config, invalid retained-log config | do not hot-loop restart; require config repair |
| `70` | daemon runtime fatal | accept-loop fatal error, bounded shutdown drain failure, unrecoverable lifecycle-control failure | supervisor should restart after bounded backoff |
| `71` | OS/transport environment failure | bind failure after ownership, endpoint cleanup failure, host I/O/resource failure | supervisor may restart, but surface host-environment fault distinctly |

### Mapping rules

- local request-level failures do not exit the process; they stay connection
  scoped
- fatal listener/runtime failures map to `70` or `71`
- configuration and startup contract violations fail closed with `64`
- unexpected invariant breaks remain `1`

### Supervisor expectations

- a host supervisor may restart on `70` and `71`
- the supervisor must not hot-loop forever on `64`
- restart policy must preserve the host-wide singleton invariant from ADR-002
- restart is a clean new process, not an in-process listener self-heal loop

### 5. Runtime SLOs

| SLO | Target | Meaning |
|---|---|---|
| Wedge recovery | `<= 1s` | shutdown request or fatal transport event must unblock the serving path promptly rather than hanging in a scoped waiter |
| Accept-error teardown | `<= 2s` | fatal accept failure must transition to typed process exit after wake, drain start, and endpoint unpublication |
| Clean shutdown | `<= 5s` | graceful daemon shutdown, including tracked request drain and background lane stop, remains bounded |
| Socket cleanup | `<= 100ms` | endpoint unpublication and local socket cleanup complete promptly after serving stops |

These SLOs are not throughput targets. They are correctness and operability
targets for daemon liveness and supervisor behavior.

### 6. Peer Transport Concerns To Track Separately

Peer transport does not share the same loop shape today because it is not a
persistent listener. The current runtime opens a fresh `TcpStream` per send
attempt and relies on retry/replay classification.

Concerns to carry separately:
- the local IPC redesign should not force peer transport into a fake
  long-lived-session abstraction it does not currently need
- peer retry, replay persistence, and outcome-unknown handling already form a
  separate state machine and should keep their own failure taxonomy
- if a future sprint introduces persistent peer sessions, that sprint should
  adopt the same "one receive loop per socket plus explicit shutdown beacon"
  rule rather than the current per-attempt exchange
- peer transport should eventually align its fatal-path reporting with the
  same exit-code taxonomy and observability vocabulary used for local IPC

### 7. ADR And Test-Policy Implications

### ADR-002

No ADR-002 reversal is needed. S.13 tightens one implication that is already
consistent with ADR-002:
- endpoint publication happens only after ownership is held
- endpoint unpublication happens before ownership release

If implementation work needs stronger wording than ADR-002 currently has,
the follow-on implementation sprint should amend ADR-002 explicitly rather than
encoding the rule only in code comments.

### No-daemon-spawn test policy

S.13 does not reopen the accepted no-daemon-spawn testing direction.
- transport design changes must stay testable through in-process seams for
  ordinary correctness
- real daemon-process coverage remains limited to the narrow runtime suite
- no new test-only daemon launch path is introduced

### 8. Implementation Slices For The Follow-On Worktree

Primary code targets:
- `crates/atm-daemon/src/local_ipc_transport.rs`
  - replace the event-channel serve orchestration with a direct accept loop
  - add the connection receive loop and shared `ShutdownBeacon` wiring
  - move endpoint cleanup behind `SocketEndpointGuard`
- `crates/atm-daemon/src/lifecycle_control.rs`
  - keep platform hooks, but expose them as shutdown/reload requests into the
    beacon-driven runtime contract
- `crates/atm-daemon/src/composition.rs`
  - map typed fatal runtime outcomes to exit codes and supervisor-facing
    observability
- `crates/atm-daemon/src/peer_transport.rs`
  - document separate follow-up concerns only; no persistent-session rewrite in
    the same sprint

Secondary implementation work:
- targeted runtime tests for accept failure, shutdown wake, endpoint cleanup,
  and bounded drain
- doc updates if ADR-002 wording needs to be strengthened

## References

- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `docs/adr/ADR-002-host-wide-daemon-singleton.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`
- `docs/phase-S/sprint-S12.md`
- `TASK-1219-PROD-REVIEW`
