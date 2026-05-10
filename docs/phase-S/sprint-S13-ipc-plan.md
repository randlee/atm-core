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

It also reconciles the specific Opus failure inventory from the follow-up
analysis at `integrate/phase-S @ 847b150`:
- accept-error lifecycle wedge
- missing Drop-based socket cleanup
- missing typed exit codes
- accept-after-terminate silent drop
- dispatch-handle retention until shutdown
- event-channel disconnect wedge
- supervisor-restart preference over internal re-bind

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
7. If shutdown is already requested when `accept()` returns a live stream, the
   connection runtime sends one typed daemon-shutting-down response when
   framing is still possible, then closes the stream. Silent reset is not the
   target behavior.

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

### Async Dispatch Ownership

- Tracked dispatch registration extends the existing daemon-private
  `request_runtime` tracked-work registry described in
  `docs/atm-daemon/architecture.md`.
- The redesigned local IPC path keeps the current single-request-per-connection
  cap. The receive loop does not introduce request multiplexing on one
  connection.
- The bounded tracked-work registry remains the authoritative ownership record
  for request workers under `REQ-DAEMON-TRANSPORT-004`.
- The receive loop may hand a validated request across an async boundary, but
  it must not create detached work outside that bounded registry contract.

### Cancellation contract

- `ShutdownBeacon` is the only accepted process-wide transport shutdown signal.
- The accept loop, lifecycle waiter, and every connection runtime must observe
  the same beacon instance.
- Connection runtimes may finish an in-flight response if they can do so within
  the bounded shutdown budget; otherwise they must fail closed with a typed
  transport/runtime error and release the socket.
- Detached, untracked request execution remains forbidden under
  `REQ-DAEMON-TRANSPORT-004`.
- Dispatch-worker handles must be reaped continuously as part of connection
  lifecycle bookkeeping rather than only after a successful response write or
  at final shutdown. The goal is to prevent bounded-but-cumulative `JoinHandle`
  retention under sustained faulted traffic.

### Why this shape

This keeps transport proof obligations local:
- listener correctness lives in one loop
- per-connection correctness lives in one loop
- dispatch correctness stays behind one async boundary
- shutdown correctness is driven by one shared signal

That is materially easier to reason about than the current
accept-thread-plus-event-channel structure.

This also removes the specific Opus `event_tx.send` / `event_rx.recv` wedge
class from the core serve path. A direct accept loop plus shared beacon makes
channel-disconnect deadlock impossible in the transport control plane because
the main serving thread no longer depends on an internal event bus to observe
accept failure or terminate state.

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
- on Unix: unlink the socket-path artifact when required
- on Windows: close the server-side pipe handle and ensure no stale endpoint
  continues to advertise daemon availability; there is no filesystem path
  artifact to delete
- expose one platform-neutral `unpublish()` contract above the adapter layer
- guarantee endpoint unpublication before daemon ownership release
- provide one cleanup path that runs on ordinary shutdown, fatal return, and
  panic unwind so stale socket artifacts are not left behind until the next
  bind attempt
- keep the platform-specific cleanup steps isolated inside the guard's OS
  implementation rather than leaking Unix socket or Windows pipe details into
  runtime orchestration

### Drop contract

1. serving state stops accepting new connections
2. connection drain begins
3. `SocketEndpointGuard` drops and unpublishes the endpoint
4. host ownership is released afterward

This ordering is required by ADR-002. The host must never re-advertise
"daemon available" by releasing `owner.lock` while the old endpoint is still
reachable or stale.

The implementation target is Drop-based cleanup rather than bind-time cleanup
alone. Bind-time stale-socket removal remains a defensive recovery step, but it
is no longer the primary cleanup mechanism.

`SocketEndpointGuard` is an RAII field owned in `RuntimeComposition` scope and
declared after the host-ownership guard so Rust field-drop order enforces the
required shutdown sequence. Last declared drops first, which means endpoint
unpublication happens before `HostOwnershipAdapter` release.

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
- a fatal accept path requested through `ShutdownBeacon` transitions
  `Running -> Draining -> Stopped`; it does not jump directly from `Running` to
  `Stopped`
- fatal listener/runtime failures map to `70` or `71`
- configuration and startup contract violations fail closed with `64`
- unexpected invariant breaks remain `1`
- the typed process exit is emitted only after the runtime reaches `Stopped`

### Supervisor expectations

- a host supervisor may restart on `70` and `71`
- the supervisor must not hot-loop forever on `64`
- restart policy must preserve the host-wide singleton invariant from ADR-002
- restart is a clean new process, not an in-process listener self-heal loop
- bind failure is not retried internally by the daemon transport. It exits with
  a typed code and lets the host supervisor apply bounded backoff.

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

### Restart model

S.13 explicitly adopts the supervisor-restart model recommended by the Opus
analysis:
- no internal IPC re-bind loop after fatal listener failure
- no in-process ownership release/reacquire cycle to recover transport faults
- no attempt to reinstall process-global lifecycle hooks in place

Rationale:
- ADR-002 singleton ownership remains simpler to prove when fatal listener
  faults lead to one typed process exit
- process-global lifecycle hook installation is not a clean hot-reload seam
- every other fatal runtime category already relies on clean restart rather
  than in-process daemon resurrection

If implementation work needs stronger wording than ADR-002 currently has,
the follow-on implementation sprint should amend ADR-002 explicitly rather than
encoding the rule only in code comments.

### No-daemon-spawn test policy

S.13 does not reopen the accepted no-daemon-spawn testing direction.
- transport design changes must stay testable through in-process seams for
  ordinary correctness
- real daemon-process coverage remains limited to the narrow runtime suite
- no new test-only daemon launch path is introduced

### LoopbackClientTransport impact

`LoopbackClientTransport` does not require direct `ShutdownBeacon` wiring for
ordinary in-process seam coverage because it does not own the local IPC accept
loop, listener wake path, or endpoint publication contract. It must continue to
exercise the same dispatcher/request-runtime boundary as the real transport, but
the beacon-owned shutdown mechanics stay specific to the local IPC server path.
That preserves the `REQ-DAEMON-TRANSPORT-001` contract that the in-process seam
backs the same dispatcher behavior without pretending to be the real endpoint
ownership implementation.

### 8. Implementation Slices For The Follow-On Worktree

Primary code targets:
- `crates/atm-daemon/src/local_ipc_transport.rs`
  - replace the event-channel serve orchestration with a direct accept loop
  - add the connection receive loop and shared `ShutdownBeacon` wiring
  - move endpoint cleanup behind `SocketEndpointGuard`
  - return a typed daemon-shutting-down result rather than silently dropping
    streams accepted after shutdown begins
  - reap tracked dispatch handles incrementally instead of only at
    post-response / final-shutdown boundaries
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

## Deferred From S.13

The Opus report included several items that are explicitly not part of the S.13
planning/implementation target unless later tasking expands scope:

- partial-write categorization improvements
  - keep this as a logging/observability follow-up, not a blocker for the core
    receive-loop redesign
- bind-failure subcoding for richer operator UX
  - S.13 requires typed exit classes and no internal retry; finer-grained bind
    diagnostics can follow separately
- composite error chaining when both serve and shutdown fail
  - useful operator signal, but secondary to removing the fatal-path wedge
- Windows eventfd-equivalent lifecycle wake primitive
  - already accepted as a documented exception rather than a mandatory S.13
    redesign item

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
