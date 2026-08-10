# ATM-Daemon Crate Architecture

> **Phase AI supersession notice:** the planned daemon has one REST router,
> reached through Unix HTTP/UDS or loopback TCP, Windows loopback TCP, and
> HTTPS/TCP remotely. Legacy Windows local transports, custom ATM frames,
> replay/retry runtime state, and separate peer handling are not
> accepted architecture for new work; ADR-033 through ADR-036 govern the
> migration.

## 1. Purpose

This document defines the `atm-daemon` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns runtime composition only.

The legacy frame protocol ICD was intentionally deleted with the AI.6 HTTP
router landing (`764bdd32`); it is not a retained or fallback contract.
Phase AI's versioned target contract is:
- [`./http-api.md`](./http-api.md)

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

The daemon observability boundary contract lives in:
- [`./observability.md`](./observability.md)

The daemon/client recovery text rule set lives in:
- [`./recovery-text-rules.md`](./recovery-text-rules.md)

This crate remains part of the current workspace.

## 1.1 ADRs

## Daemon is the current runtime composition root

```yaml
adr_id: ADR-ATM-DAEMON-001
crate: atm-daemon
title: Daemon is the current runtime composition root
status: superseded
superseded_by: ADR-ATM-RUNTIME-001
date: 2026-05-03
deciders:
  - team-lead
  - arch-ctm
tags:
  - composition
  - runtime
related_boundaries:
  - BOUNDARY-ServerTransport-Socket
  - BOUNDARY-RequestDispatcher-Daemon
code_references:
  - docs/atm-daemon/boundaries.md
  - docs/atm-rusqlite/boundaries.md
```

Context:
- The current crate set has no separate composition/app crate, but the runtime
  still needs one legal owner that can assemble concrete adapters.

Decision:
- `atm-daemon` is the production runtime composition root in the current Phase
  R design line.
- It may assemble concrete runtime and store adapters while remaining thin and
  business-logic-free.

Consequences:
- The runtime has one legal place to wire concrete adapters.
- Forbidden dependency rules can still keep CLI and thin extensions away from
  daemon and SQLite internals.

Alternatives considered:
- Leave composition ownership unspecified.
- Add a separate composition crate before the boundary line is stable.

Follow-up work:
- Keep adapter assembly in daemon-owned composition code only.
- Revisit only if a later ADR extracts a dedicated composition crate.

Phase-AA supersession note:
- this ADR records the current merged ownership shape only
- `Phase AA` intentionally supersedes it by moving concrete runtime/store
  composition into a dedicated `atm-runtime` crate
- `AA.2` lands that transfer for production composition paths
- after `Phase AA`, `atm-daemon` is no longer a legal home for SQLite adapter
  construction

## 2. Responsibilities

The `atm-daemon` crate is responsible for:

- singleton daemon startup and ownership checks
- local daemon API listener
- remote daemon-to-daemon transport listener/client
- runtime wiring of `atm-core` service boundaries
- live agent-status cache
- direct post-send emission routing for local and graft-backed recipients
- daemon/runtime observability emission
- daemon health/status query surface for `atm doctor`

The `atm-daemon` crate must remain thin.

Phase-AA target direction:
- the daemon remains transport/lifecycle-owned
- `AA.2` moves concrete production runtime/store composition into
  `atm-runtime`
- SQLite-specific composition, observability, replay, and direct store-health
  logic are removed from this crate
- daemon health becomes daemon-owned runtime projection only
- subsystem-owned diagnostic traits perform backend-specific investigation
- daemon doctor code aggregates subsystem reports and daemon-owned runtime
  state only, and may compare reports for drift without reimplementing backend
  diagnosis
- in the steady-state `Phase AA` ownership split, `atm-daemon` owns only:
  1. transport admission and endpoint publication
  2. lifecycle / singleton ownership and bounded shutdown
  3. request validation and frame-contract enforcement
  4. request dispatch through injected service/runtime ports
  5. typed daemon error/report projection for daemon-owned runtime state
- the aggregate-only doctor surface consumes `MailStoreDoctor`,
  `RosterStoreDoctor`, and `ConfigDoctor`

Phase R redesign notes:
- `atm-daemon` remains runtime-oriented, not business-logic-oriented
- `atm-daemon` is the current runtime composition root for production wiring
- remote daemon-to-daemon client behavior uses the same shared protocol and
  client/server transport contract family rather than a separate daemon-only API

Current packet-supported daemon surface:
- send compose
- send acknowledge
- receive
- clear
- doctor
- heartbeat

Receiver-handoff rule:
- the daemon may emit one post-send event after durable message commit
- receiver-specific delivery details stay behind receiver implementations such
  as local tmux and `atm-graft`
- when the retained built-in helper is used directly, it must consume one
  pre-resolved `ATM_INTERNAL_NUDGE` envelope rather than re-querying template
  override state from inside daemon-owned code; the shipped daemon path
  otherwise resolves, renders, and delivers built-in nudges in-process
- the accepted daemon architecture must not require daemon-owned graft session
  registration, per-session nudge queues, fetch/drain inspection, or a
  dedicated advisory-stream request family
- transport receive loops remain thin request dispatch paths rather than
  homes for receiver-specific session behavior

Current retained ATM surfaces outside the daemon request/response packet family:
- `atm log`
- `atm teams`
- `atm members`

## 3. Architectural Rules

- `atm-daemon` must not reimplement `atm-core` business logic.
- `atm-daemon` must not access SQLite except through the `atm-core` store
  boundary.
- `atm-daemon` must not own concrete SQLite semantics.
- `atm-daemon` must not parse or write Claude mailbox JSON on the accepted
  runtime.
- `atm-daemon` must not resolve caller-owned command identity from daemon
  ambient environment, hook files, or repo-local config.
- caller-owned request packets received by `atm-daemon` must already carry
  resolved caller identity as required request data, and the daemon must reject
  any request shape that violates that contract.
- write-affecting daemon mail events must keep one direct post-persist rule:
  persist first, then emit post-send behavior only when the recipient exposes
  that capability
- daemon runtime-health/status assembly must discover teams and members only
  through the installed `RosterStore`; `ATM_HOME/.claude/teams` is a config
  ingress surface, not a runtime-truth discovery path
- deep backend-specific diagnosis belongs to subsystem-owned doctor traits
  rather than daemon-local logic
- daemon doctor aggregation may combine subsystem reports with daemon-owned
  runtime state and compare those reports for drift, but it must not inspect
  SQLite internals directly
- read-mostly daemon runtime-health/status projection must publish immutable
  snapshots to readers rather than coordinating ordinary reads through one
  daemon-shared mutable cache lock
- daemon worker lanes with active queue/debounce/completion state must use one
  worker-owned command-channel or actor ownership model rather than exposing
  shared mutable coordination locks to callers
- daemon watch/reconcile lanes are historical only and are not part of the
  accepted runtime architecture
- Historical through AI.5 only: the daemon owned custom-frame transport
  implementations. The accepted Phase AI line is one HTTP router reached by
  UDS/loopback TCP locally, HTTPS remotely, and an in-process HTTP adapter in
  tests. No custom frame header or `LoopbackClientTransport` may be retained as
  a fallback.
- the accepted same-host Windows local-IPC depth contract is fail-fast and
  unary:
  - a dispatcher panic during shutdown must record one panic-path failure and
    complete bounded shutdown without hanging the serve path
  - an injected listener accept failure must record one accept-path failure and
    terminate the affected serve path with a typed bounded error; the daemon
    must not invent retry/backoff behavior for that injected failure contract
  - a new connection attempt after terminate must fail quickly with a typed
    shutdown/unavailable outcome rather than hanging on a dead endpoint
- same-host local IPC endpoint naming and same-user access-control semantics
  must be owned by the local-IPC adapter rather than by callers constructing
  platform-specific socket paths, pipe names, or ACL details
- the shared endpoint helper may publish a Windows loopback-TCP endpoint record
  and local capability from the logical ATM endpoint contract; callers must
  consume that record rather than derive a path, port, or capability
- same-host daemon functionality must ship with feature parity on every
  supported operating system; Windows is not a compile-only or degraded-host
  target
- same-host daemon hosting uses one user-scoped background daemon model on
  macOS, Linux, and Windows; service-control integration may exist inside the
  Windows lifecycle adapter, but Phase S parity does not depend on a separate
  SCM-only host model
- client-specific runtime logic is owned by the client crate; `atm-daemon`
  may serve the advisory transport but must not own embedded client receive
  behavior
- same-host transport and lifecycle control must remain platform-neutral above
  the adapter line:
  - platform-specific listener/stream/control types are allowed only inside
    owned adapter modules
  - runtime composition, dispatcher, bounded canonical-record recovery, status cache, and runtime lanes
    must not depend directly on Unix-only host APIs
- cross-host delivery is daemon-to-daemon only.
- remote send success is defined by remote daemon acceptance for that one
  request. Failure returns one typed transport error and creates no retry
  state. ADR-038's explicit bounded canonical-record scan is the sole recovery
  mechanism.
  failures:
  - timeout
  - connection refused
  - connection reset / aborted
  - broken pipe
  - host unreachable / network unreachable
- non-retryable peer failures include protocol/frame corruption, TLS or
  certificate mismatch, authentication mismatch, and explicit remote daemon
  rejection
- if the request body has been fully written but remote acceptance has not been
  confirmed when the connection drops, the runtime returns one typed transport
  failure and does not guess success or create delivery state
- outbound peer attempts resolve and dial per attempt so ordinary interface
  changes on the sender host do not require daemon restart
- inbound TCP/TLS listeners should bind wildcard/unspecified addresses by
  default; ordinary cable unplug / replug or Wi-Fi to ethernet rebinding must
  not require restart in that default mode
- if the configured listener bind address is an explicit local IP that later
  disappears or changes, the runtime must enter degraded status and require
  bounded reload/rebind via the runtime reload path
- graceful shutdown finalization must remain bounded; best-effort SQLite WAL
  checkpoint and observability flush steps must time out rather than block
  daemon exit indefinitely
- startup does not run a replay-resume sweep or require a SQLite-backed replay
  store. ADR-038's explicitly requested, bounded canonical record scan is the
  only reconciliation behavior and uses storage traits after peer success.
- daemon runtime failures must remain typed and must not depend on
  panic/unwrap for routine transport, socket, or store-boundary failure.
- daemon observability remains structured through `sc-observability`; no ad hoc
  debug-only runtime path replaces it in production.
- daemon observability is bottom-of-stack:
  - the shared daemon observability layer imports no daemon subsystem types
  - daemon subsystems emit already-shaped daemon event payloads through the
    injected trait
  - central daemon observability must not reconstruct subsystem semantics after
    the fact
- plugin-local observability does not replace daemon-owned runtime/transport
  sinks; daemon-owned events stay daemon-owned.
- daemon retained-log reporting must use the host-scoped ATM log contract:
  `~/.atm/logs/atm.log.jsonl` by default and `ATM_LOG_DIR` when overridden.
- daemon health/reporting must not point retained-log status at `.local/share`,
  `~/logs`, `~/.claude/logs`, or other non-ATM-owned defaults.
- the default retained daemon logging baseline must keep:
  - daemon lifecycle `info!` events
  - every daemon/runtime/transport `warn!` event
  - every daemon/runtime/transport `error!` event
- runtime subsystems stay fully isolated:
  - SQL/store calls belong only to the store boundary
  - post-send emission belongs only to the post-send/advisory boundary
  - local-IPC and network I/O belong only to the transport boundary
- UDP is not an approved daemon control-plane transport for same-host CLI
  request/response traffic; same-host and remote request families use the
  shared HTTP request/response contract, carried by UDS or loopback TCP
  locally and HTTPS/TCP remotely
- Phase AD note:
  - watcher/reconcile compatibility-projection behavior is retired
  - daemon architecture no longer depends on `ADR-010`

## 3.0.1 Allowed Operating-System Difference Inventory

The Phase S production target allows OS-specific implementation differences
only in these daemon-owned areas:

1. Same-host local IPC transport
   - Unix: Unix domain socket
   - Windows: loopback TCP HTTP local IPC
2. Runtime lifecycle-control source
   - Unix: signal-based control source
   - Windows: console or service-control source
3. Host ownership
   - Unix and Windows may differ in file-locking and owner-record mechanics,
     but must preserve the same singleton, stale-owner, and teardown behavior

Everything else must remain platform-neutral:
- request parsing and dispatch
- handler behavior
- daemon status cache and doctor projection
- bounded canonical-record recovery, deadline, and timeout semantics
- direct post-send emission routing and typed warning propagation
- shutdown ordering and typed error surfaces

If a code path needs additional platform branching outside the three areas
above, the architecture docs and boundary inventory must be updated before the
implementation is accepted.

## 3.0.2 Historical Shared Frame Contract

Through AI.5, the daemon host shell used the shared ATM frame contract. AI.6
retired that contract for ADR-033's HTTP contract; the frame contract must not
be extended or preserved as a fallback.

That includes:
- one fixed ATM frame header
- one packet-kind registry
- one request/response DTO family
- one typed protocol-failure model
- exact `magic`, `version`, `flags`, and `request_id` rules
- exact `message_kind` numeric assignments for the current daemon packet
  surface

Phase S carries forward the current PID continuity model unchanged. Windows
parity work may reimplement how liveness is enforced inside the host-ownership
or lifecycle adapters, but it must not silently redesign PID semantics without
an explicit follow-up ADR.

## 3.1 Singleton Runtime

Hard invariant:
- it must be impossible for more than one `atm-daemon` process to exist
  anywhere on the host for the supported runtime model

Architectural rule:
- singleton enforcement belongs in the runtime wrapper only
- the runtime must fail closed rather than allowing split ownership
- singleton enforcement must use multiple layers:
  - a pre-spawn launch gate before client-side fork/exec
  - a daemon-side startup gate before serving state
  - a repository lint/CI gate that prevents ordinary tests from designing
    around the runtime invariant
- no alternate socket path, alternate `ATM_HOME`, or test-only helper is an
  exception to the singleton rule
- the host-wide ownership root is `~/.atm/daemon/` derived from the OS user
  home, not from `ATM_HOME` or the serving socket path
- client-side launch admission uses the stable lock file
  `~/.atm/daemon/launch.lock`
- daemon-side serving admission uses the stable lock file
  `~/.atm/daemon/owner.lock`
- Phase S host ownership uses one cross-platform whole-file exclusive-lock
  contract on those stable file paths rather than lock-file creation/deletion
  as the ownership signal
- lock acquisition must use non-blocking `FileExt::try_lock_exclusive` for
  both `launch.lock` and `owner.lock`; blocking `lock_exclusive` is not the
  accepted serving-admission contract
- failed non-blocking acquisition of either lock file must surface one typed
  `already_owned` admission outcome rather than silently spinning or blocking
- the current S.1 extraction uses the existing `fs2` file-locking crate while
  preserving the required non-blocking whole-file lock semantics
- owner-visible metadata is the lock-file contents in documented
  `pid[:token]` form while the exclusive lock is held
- lock files must live on a local filesystem with working host-local advisory
  lock semantics; network-mounted or NFS-backed `~/.atm/daemon/` roots are not
  a supported singleton deployment configuration
- if an exclusive lock on `owner.lock` can be acquired, startup may inspect
  and replace stale owner metadata under that held lock; if recovery cannot
  safely claim the same lock path, startup must fail with
  `ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED`
- launch-to-owner handoff sequence is:
  1. the client acquires and holds `launch.lock` before fork/exec
  2. the daemon process acquires `owner.lock` with `try_lock_exclusive`
     before publishing any same-host endpoint or entering serving state
  3. once the daemon confirms serving state, the launcher releases
     `launch.lock`
  4. if the daemon cannot acquire `owner.lock`, startup fails closed and the
     launcher must release `launch.lock` without retrying a second daemon
- Windows uses the same logical handoff and typed `already_owned` admission
  outcome even though the underlying file-locking calls and error codes differ

Lifecycle state model:
- the daemon runtime must explicitly model:
  - `Starting`
  - `Running`
  - `Draining`
  - `Stopped`
- the authoritative transition document is
  [`./startup-state-machine.md`](./startup-state-machine.md)
- the implementation must use typestate as the governing lifecycle contract;
  helper enums may exist internally, but they must remain subordinate to the
  typestate boundary and must not replace the explicit legal transitions
- accepted transition graph:
  - `Starting -> Running`
  - `Starting -> Stopped` on failed startup/rollback
  - `Running -> Draining`
  - `Draining -> Stopped`
- illegal transitions such as `Running -> Starting` or `Stopped -> Running`
  without reinitialization must be prevented by the runtime boundary
- Historical (retired by AM.3): `RuntimeComposition::start()` was the legacy
  daemon bootstrap entrypoint. The active lifecycle enters through
  `atm_daemon_bootstrap::run_replacement_daemon_with_observability`.
- any post-`Running` exit path, including listener/accept failures, must pass
  through `Running -> Draining -> Stopped` rather than silently forcing
  `Running -> Stopped`
- Phase S currently models fatal accept failure as a conventional
  `ServeEvent::AcceptError` control-plane event rather than as a dedicated typed
  lifecycle-transition enum; that shape is accepted so long as the runtime
  still transitions through `Running -> Draining -> Stopped` afterward
- repeated lifecycle-control installs for one platform implementation must
  reuse the same process-wide control flags without clearing a pending
  terminate/reload bit between installs
- the process-global lifecycle wake worker is still explicit runtime-owned
  state:
  - one worker instance may be reused across repeated installs while the daemon
    keeps the same process-global lifecycle adapter alive
  - runtime teardown must request worker stop, unregister the active signal
    hooks, and join the worker within the documented `1s` bound
- Windows keeps one accepted lifecycle polling exception:
  - `signal_hook::flag` exposes no matching blocking wake primitive for the
    console/service-control adapter
  - the lifecycle wake worker may therefore poll at `25ms` intervals on Windows
    only
  - that polling loop must stay isolated to `lifecycle_control.rs` and remain
    under the same explicit shutdown/join contract as the Unix wake worker

Privacy boundary:
- the lifecycle state type and transport/runtime adapter internals remain
  crate-private implementation details
- public callers interact through daemon request/response surfaces and health
  queries, not through direct state mutation
- transport submodules expose only the listener/client boundary types required
  for runtime composition; frame codecs, connection state, and transport
  helpers remain crate-private
- dispatcher submodules expose only the dispatcher trait/boundary and typed
  request/response contracts; routing tables and handler wiring remain
  crate-private
- status-cache submodules expose only the boundary needed for daemon health and
  routing decisions; cache internals and mutation helpers remain crate-private
- post-send receiver-handoff submodules expose only the owned post-send
  boundary traits or façades required by runtime composition; delivery
  internals remain
  crate-private
- observability submodules expose only the daemon-owned event sink façade used
  by runtime composition; sink plumbing and field-shaping helpers remain
  crate-private

## 3.1.0 Daemon Observability Boundary

The final daemon observability contract is defined in
[`./observability.md`](./observability.md).

Required architectural decisions:
- the injected daemon observability trait remains object-safe and sealed
- the daemon lifecycle stays modeled as an explicit typestate-backed runtime
  state machine; helper enums may not replace the typestate contract
- `LaunchGateGuard` remains a launch/admission coordination primitive, not a
  lifecycle typestate token, but it still carries one type-level invariant:
  a live guard proves launch admission is held for the current startup handoff
  and cannot coexist with the post-admission running token
- daemon event payloads use typed semantic identifiers:
  - `DaemonSubsystem` enum
  - `AtmMessageId`
  - `TaskId`
- `team`, `agent`, `sender`, `recipient`, `message_id`, and `task_id` are
  event payload fields, not injected logger state

V.2 migration targets:
- `daemon_runtime_observability.rs`
- `daemon_observability.rs`
- `runtime_health.rs`
- `local_ipc_transport.rs` (retired by AM.3)
- `advisory_runtime.rs`
- `peer_transport.rs`
- `host_ownership.rs`
- `lifecycle_control.rs`
- `runtime_status_cache.rs`

V.3 deletion targets:
- `emit_runtime_event(...)`
- `map_command_event(...)`
- `map_runtime_event(...)`
- any central daemon helper path that exists only to reconstruct subsystem
  meaning after the fact

Transport dispatcher rule:
- local-IPC and TCP/TLS listener/connection receive loops are deliberately tiny
- they may:
  - read a framed request
  - parse a qualified request type
  - dispatch through one injected dispatcher boundary
  - encode a typed response
- they may not:
  - run SQL directly
  - invoke historical watch/reconcile logic directly
  - emit notifications directly
  - embed workflow/business-state transitions
- the same dispatcher/handler contract must back the in-process `test-socket`
  transport so handler behavior is testable without Unix-specific or TCP/TLS
  host code
- same-host functional coverage must also exercise the real local-IPC adapter
  on Unix and Windows through one shared harness shape; a Unix-only host test
  suite is not sufficient for Phase S closeout
- the landed S.4 coverage audit is anchored by:
  - `crates/atm-daemon/src/tests.rs::local_ipc_runtime_round_trips_doctor_requests_on_shared_transport`
  - `crates/atm-daemon/src/tests.rs` host-ownership coverage
  - `crates/atm-daemon/src/lifecycle_control.rs` Windows lifecycle tests
  - `crates/atm/src/composition.rs` same-host client and launch-gate tests

Dispatcher/handler rule:
- request-kind routing belongs to the dispatcher boundary, not to the socket
  adapter
- concrete request-family behavior belongs to injectable handlers behind that
  dispatcher
- same-host local-IPC and TCP/TLS adapters share the same dispatcher/handler
  contract
- the dispatcher itself stays thin and must not absorb request-family business
  logic

## 3.1.1 Internal Partitioning

The daemon runtime is one crate but it is not one architectural blob.

Accepted daemon-private partitions:
- `ownership`
  - owns host-wide lock paths, owner-record reads/writes, stale-owner recovery,
    and singleton cleanup rules
- `server_runtime`
  - owns listener bootstrap, accept loop, connection registry, drain
    sequencing, and forced-cancel escalation
- `request_runtime`
  - owns per-connection request execution, request-work tracking, request
    deadlines, and response emission
  - performs only request validation, one canonical storage transaction, one
    post-commit work signal, and response emission; it must not scan peer
    records, wait on peer work, or perform DNS/socket/TLS/HTTP/hook/nudge work
- `runtime_status`
  - owns the live status cache, cache-cap semantics, roster hydration,
    reload-time runtime-view assembly, and doctor-health projection into
    `atm doctor`
  - reader projection uses immutable snapshot publication rather than shared
    mutable cache locking
- `peer_http_adapter`
  - owns HTTP(S) socket/TLS adaptation only; it cannot persist, queue, retry,
    route, or nudge
- `peer_recovery`
  - owns ADR-038's bounded non-durable independent peer jobs,
    canonical-record query handoff, backoff, and status-event emission; it
    cannot own a message payload, cursor, receipt, attempt history, or storage
    implementation, and it makes no same-peer FIFO/stream promise

Historical-only retired partitions:
- `watch_runtime`
- `reconcile_runtime`

Phase `AD` rule:
- these retired lanes may survive temporarily only as deletion scaffolding
- they are not part of the accepted daemon runtime architecture
- no new architecture text may describe them as required production partitions

Observability rule:
- daemon-owned `sc-observability` sinks are a cross-cutting runtime facility
  used by all partitions as needed
- observability is not a ninth partition and must not become a backdoor for
  bypassing the partition ownership lines above

Required ownership rules:
- `lib.rs` is the crate entrypoint and daemon-private integration seam only; it
  must not remain the long-term home for singleton ownership, server runtime,
  request execution, and shutdown policy simultaneously
- singleton cleanup must remain ownership-safe:
  - the runtime must clear or invalidate current owner metadata while the live
    exclusive lock is still held
  - the stable `launch.lock` and `owner.lock` paths must not be relied on as
    ephemeral ownership sentinels that are deleted during normal handoff
  - the runtime must not release the ownership lock while current owner
    metadata still presents that daemon as authoritative
- request work launched from the server runtime must remain owned by runtime
  drain accounting until it finishes or is cancelled
  - `request_runtime` owns one runtime-private tracked-work registry keyed by
    accepted request execution units
  - the registry must stay bounded by the current transport contract:
    - Phase R remains single-request-per-connection, so one accepted
      connection contributes at most one active request-work entry
    - the documented `32` per-connection in-flight cap remains the resource-cap
      contract for a later framed-multiplexing extension, not the current
      thread shape
  - shutdown/drain clears tracked request work only after the request finishes
    or a forced-cancel path has run
- background-lane startup and shutdown must remain rollback-safe and must not
  stop after the first lane error if more cleanup is still possible
  - if lane startup fails after earlier lanes have already started, cleanup
    runs in reverse start order until every started lane has been asked to stop
  - after partial-start cleanup, the runtime must hold no lane-specific worker
    ownership before it returns the startup failure
- bounded caches must be bounded in actual retained cardinality, not only by
  state demotion labels

## 3.1.2 Graceful Shutdown

Shutdown is part of the daemon contract, not an implementation detail.
`R.18` landed the runtime-ops behavior set, and `R.20` hardens the internal
partitioning and enforcement rules needed to keep that behavior maintainable.

Required shutdown sequence:
1. stop accepting new local and remote connections
2. mark the runtime as draining so new work fails clearly
3. allow inflight work to finish within the drain deadline
4. cancel remaining inflight work at the force-cancel deadline
5. checkpoint SQLite WAL
6. flush observability sinks on a best-effort basis
7. request stop for the lifecycle wake worker, unregister active hooks, and
   join the worker within the documented `1s` bound
8. clear or invalidate current owner metadata while `owner.lock` is still held
9. release the live exclusive ownership lock
10. remove the same-host listener artifact if the local-IPC adapter requires a
   removable endpoint artifact on that operating system

Force-cancel rule:
- the forced-shutdown path must interrupt blocked socket reads and writes via
  connection shutdown rather than falling through to `process::exit(1)`
- failure to drain within the force deadline is reported as a typed runtime
  failure after interrupting active connections

### Runtime SLOs

| SLO | Target | Contract |
|---|---|---|
| Wedge recovery | `<= 1s` | fatal transport events or shutdown beacons must unblock the lifecycle waiter and serving path promptly |
| Accept-error teardown | `<= 2s` | a fatal local-IPC accept failure must reach typed runtime exit after drain start and endpoint unpublication |
| Clean shutdown | `<= 5s` | orderly daemon shutdown, including tracked request drain and background-lane stop, stays bounded |
| Socket cleanup | `<= 100ms` | same-host endpoint unpublication completes promptly once serving stops |

Exit-code expectations tied to these SLOs:
- bind failure exits `70` and relies on supervisor restart/backoff rather than
  in-process rebind
- singleton or stale-owner admission failures exit `64` and must not hot-loop
  restart
- lifecycle-wedge detection exits `71`
- degraded shutdown after force-cancel or helper-lane timeout emits
  `DaemonShutdownDegraded` and exits `72`

Required deadlines:
- normal drain deadline: `5s`
- force-cancel deadline after drain starts: `10s` total

Ordering rule:
- singleton ownership is released only after listener shutdown and checkpoint
  sequencing completes or the runtime has failed closed
- owner metadata must be cleared or invalidated before exclusive-lock release;
  publishing a current owner record after unlock is a contract violation

## 3.1.3 Signal Handling

Required runtime-control mappings:
- Unix may use:
  - `SIGINT`: begin graceful shutdown
  - `SIGTERM`: begin graceful shutdown
  - `SIGHUP`: trigger bounded configuration / roster rescan without dropping
    singleton ownership
- Windows may map the same logical control events through console or service
  control equivalents
- the accepted Phase S console mapping is:
  - `SIGINT` / console terminate event: begin graceful shutdown
  - `SIGTERM` equivalent terminate event: begin graceful shutdown
  - `SIGBREAK` / `CTRL_BREAK_EVENT`: trigger bounded configuration / roster
    rescan while retaining singleton ownership

Architectural rules:
- the lifecycle-control source installs before any listener begins accepting
- control-triggered shutdown uses the same drain/checkpoint/release path as an
  explicit runtime stop on every supported host platform
- reload/rescan validates candidate configuration before it replaces the active
  runtime view; invalid configuration yields a typed reload error and
  preserves the last known-good serving configuration
- ADR-006 records the bounded reload delivery decision and the required
  last-known-good preservation semantics
- singleton ownership artifacts must be released on normal signal-driven exit
  and retained only on crash/fail-stop paths where the process cannot run
  cleanup code

## 3.2 Resource Caps And Saturation

The daemon must use explicit, small resource ceilings.

Required caps:
- max concurrent accepted connections: `64`
- max per-connection inflight requests: `32`
- ingest queue depth: `1024`
- post-commit work queue depth: `256`
- active peer jobs: `64` globally and `8` per host
- peer job deadline: one absolute `10s` DNS-through-response budget; no
  independent per-leg deadline
- SQLite handle/pool budget: min `1`, max `4`
- live status-cache cap: `4096` entries

Required saturation behavior:
- connection cap exceeded: reject new accepts with a typed over-capacity error
- per-connection inflight exceeded: reject excess requests on that connection;
  in Phase R the transport remains single-request-per-connection, so the
  in-flight count is structurally `1` until framed multiplexing exists
- ingest queue full: fail the enqueue with structured degradation/health
  reporting through `DaemonIngestQueueSaturated`; no silent drop
- status-cache cap exceeded: evict least-recently-updated noncritical entries
  from the live-member map so the retained map cardinality remains bounded;
  removed entries project as explicit `unknown` on later snapshot/doctor reads
  and still emit structured warning output

## 3.3 Status Ownership

The daemon owns the live runtime view of agent status.

Architectural rules:
- live status remains in daemon memory
- current agent `pid` is transient daemon-owned runtime state and is retained
  in daemon memory as the primary liveness field
- `last_active_at` remains in daemon memory alongside live status
- daemon-managed team-member fields update only through the documented heartbeat
  socket handler in `docs/team-member-state.md`
- SQLite does not own live status, `last_active_at`, or the current process
  `pid`
- status cache rebuild after restart hydrates configured roster members as
  `unknown` and refreshes thereafter through runtime events rather than through
  persisted pid state
- startup hydration records explicit `unknown` entries in the daemon cache so
  bounded eviction can demote live members back to `unknown` without silently
  deleting the member from runtime state
- live pid conflict detection is cache-first after startup hydration; a
  live-old-pid/new-pid collision persists `identity_conflict` state in daemon
  memory until admin takeover or dead-pid retry clears it
- read-time overlays such as `active 3 seconds ago` or `idle for 30 minutes`
  are derived from daemon-memory `last_active_at`, not from durable roster
  rows
- until `schooks 1.0` is released, pid/activity updates may arrive through the
  installed Python hooks from `../agent-team-mail`
- after `schooks 1.0` is released, `schooks` becomes the controlled hook
  environment layer and reports pid/activity updates to `atm-daemon`

## 3.4 Timeouts

Required timeout defaults:
- same-host daemon request deadline: `3s`
- remote synchronous wait deadline: `10s`
- SQLite `busy_timeout`: `5000ms`
- ingest batch processing slice: `2s` max before yielding
- daemon health query used by `atm doctor`: `3s`
- lifecycle wake-worker join during runtime teardown: `1s` max
- retained-log flush and sync during runtime teardown: `2s` best-effort max
- configurable timeout overrides may raise these defaults, but
  they must not violate the floor contract:
  - global minimum timeout floor: `250ms`
  - same-host request and daemon-health minimum floor: `1s`

TCP/TLS connect, handshake, request, and read/write activity consume the one
absolute remote request deadline. They have no independent timeout floor or
ceiling that can outlive or override that deadline.

Shutdown sub-deadline rationale:
- these per-component bounds sit under the existing daemon shutdown ceilings so
  no single helper lane can consume the full runtime teardown budget alone
- timeout expiry must return typed degraded shutdown state rather than silently
  detaching helper ownership

## 3.5 Test Strategy

The daemon is not the core test strategy.

Architectural rules:
- `atm-daemon` should be testable primarily through in-process harnesses and
  fakes around its adapters
- most tests must not depend on:
  - daemon spawn
  - socket publication timing
  - retry sleeps
  - environment mutation races
  - auto-start side effects
- if process-level daemon runtime tests exist, they must remain small,
  separate, and limited to true daemon-runtime requirements
- no core ATM correctness rule should require a real daemon process for normal
  validation
- `atm doctor` and other daemon-querying CLI flows must rely on explicit daemon
  request/response paths, not private inspection shortcuts

Doctor health contract distinction:
- liveness answers whether the daemon process is present and still owns the
  runtime
- readiness answers whether the daemon is accepting requests and able to serve
  them through the documented request boundary
- `atm doctor` must report both dimensions explicitly rather than treating
  process existence as equivalent to request-serving readiness
- `AA.3` final state:
  - `RuntimeStatusSnapshot` now carries only daemon-owned runtime state and no
    store-specific readiness fields
  - SQLite/store readiness moves to subsystem doctor reports and does not
    remain part of the daemon runtime DTO
  - the daemon aggregates subsystem doctor reports plus daemon-owned runtime
    findings, but does not perform backend-specific investigation logic
- `AA.4` final state:
  - `atm-daemon` no longer imports `atm-rusqlite` directly in production code
  - daemon-side SQLite observability glue is deleted rather than retained as a
    private adapter layer
  - daemon tests that need concrete SQLite-backed state assemble through
    `atm-runtime` / daemon test helpers instead of calling SQLite boundary
    assembly functions directly
  - the earlier `AA.0` to `AA.2` transitional fields (`sqlite_ready`,
    `sqlite_detail`, and the interim degraded-ingest stub) are historical only
    and must not be reintroduced on this branch

## 3.6 Crash Recovery

Crash recovery preserves committed local mailbox truth. It does not persist
remote-delivery progress, retry rows, or an outbox. The only permitted remote
recovery is ADR-038's bounded query of existing immutable local records through
storage traits; the in-memory per-host lease, cursor, connection, backoff, and
observability projection disappear on daemon exit. A later eligible run starts
from the canonical records and their original ULIDs, never from a replay row.

WAL checkpoint and observability flush on graceful shutdown are best effort and
bounded; crash recovery cannot depend on either completing.

## 4. ADR Namespace

The `atm-daemon` crate uses the `ADR-DAEMON-*` namespace.

Initial use cases:

- singleton runtime enforcement
- local transport adapter structure
- remote daemon-to-daemon protocol structure
- direct post-send/advisory routing structure
