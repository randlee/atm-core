# ATM-Daemon Crate Architecture

## 1. Purpose

This document defines the `atm-daemon` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns runtime composition only.

The canonical daemon wire contract lives in:
- [`./protocol-icd.md`](./protocol-icd.md)

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

This crate was introduced on the Phase Q implementation line and remains part
of the current workspace.

## 1.1 ADRs

## Daemon is the current runtime composition root

```yaml
adr_id: ADR-ATM-DAEMON-001
crate: atm-daemon
title: Daemon is the current runtime composition root
status: accepted
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
  - BOUNDARY-WatchEventSource-File
  - BOUNDARY-ReconcileCoordinator-Daemon
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

## 2. Responsibilities

The `atm-daemon` crate is responsible for:

- singleton daemon startup and ownership checks
- local daemon API listener
- remote daemon-to-daemon transport listener/client
- runtime wiring of `atm-core` service boundaries
- live agent-status cache
- watch/reconcile runtime loop
- daemon/runtime observability emission
- daemon health/status query surface for `atm doctor`

The `atm-daemon` crate must remain thin.

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

Current retained ATM surfaces outside the daemon request/response packet family:
- `atm log`
- `atm teams`
- `atm members`

## 3. Architectural Rules

- `atm-daemon` must not reimplement `atm-core` business logic.
- `atm-daemon` must not access SQLite except through the `atm-core` store
  boundary.
- `atm-daemon` must not parse or write inbox JSONL except through the
  `atm-core` ingress/export boundaries.
- `atm-daemon` owns runtime implementations of one shared ATM protocol with
  multiple transport implementations:
  - cross-platform local IPC for same-host daemon access
  - TCP/TLS
  - in-process `LoopbackClientTransport` (`test-socket`)
- same-host local IPC and cross-host daemon transport must use one shared ATM
  frame header and request/response packet family rather than separate local
  and remote message systems, as defined by `protocol-icd.md`
- same-host local IPC endpoint naming and same-user access-control semantics
  must be owned by the local-IPC adapter rather than by callers constructing
  platform-specific socket paths, pipe names, or ACL details
- the shared endpoint helper may derive a Windows named-pipe path from the
  logical ATM endpoint contract, but that mapping must stay inside the
  same-host transport boundary and remain identical for the daemon and CLI
- same-host daemon functionality must ship with feature parity on every
  supported operating system; Windows is not a compile-only or degraded-host
  target
- same-host daemon hosting uses one user-scoped background daemon model on
  macOS, Linux, and Windows; service-control integration may exist inside the
  Windows lifecycle adapter, but Phase S parity does not depend on a separate
  SCM-only host model
- same-host transport and lifecycle control must remain platform-neutral above
  the adapter line:
  - platform-specific listener/stream/control types are allowed only inside
    owned adapter modules
  - runtime composition, dispatcher, replay, status cache, and runtime lanes
    must not depend directly on Unix-only host APIs
- cross-host delivery is daemon-to-daemon only.
- remote delivery may use bounded transient retry for short intermittent
  failures, but not a durable long-lived remote outbox.
- remote send success is defined by remote daemon acceptance within the bounded
  retry window.
- bounded transient retry uses exponential backoff with jitter, an initial
  delay of 250ms, a per-attempt maximum of 5s, jitter of +/-20%, and a hard
  total retry ceiling within the documented timeout budget; it must not
  collapse into fixed sleeps, unbounded churn, or tests that can wait
  indefinitely for eventual success
- retryable peer failures are limited to transient pre-acceptance socket
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
  confirmed when the connection drops, the runtime returns one typed
  `RemoteDeliveryOutcomeUnknown` failure (`ATM_REMOTE_OUTCOME_UNKNOWN`) and
  hands recovery to bounded replay/re-export rather than guessing success
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
- startup must run one bounded replay-resume sweep from the host-scoped SQLite
  state root before serving requests so pending remote handoff rows keyed by
  durable `message_key` are retried or retained with typed degraded status
- daemon runtime failures must remain typed and must not depend on
  panic/unwrap for routine transport, socket, or store-boundary failure.
- daemon observability remains structured through `sc-observability`; no ad hoc
  debug-only runtime path replaces it in production.
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
  - file-watch/reconcile logic belongs only to the watcher/reconcile boundary
  - notification delivery belongs only to the notifier/plugin boundary
  - local-IPC and network I/O belong only to the transport boundary
- UDP is not an approved daemon control-plane transport for same-host CLI
  request/response traffic; same-host and remote request families require the
  shared framed stream contract
- watcher/reconcile adapters remain crate-private and dispatch through owned
  ingress/service handlers rather than touching store/transport/notifier
  internals directly
- watcher/reconcile observation of ATM-authored compatibility projection
  updates must be idempotent for the same logical message; re-observing the
  same retrieval-stub projection must not create a new-mail churn loop
- ADR reference:
  - `ADR-010`
- daemon-owned ingress/export boundary tests must therefore preserve the same
  logical identity fingerprint across full-body and retrieval-stub projections
  for one ATM-authored message id rather than treating the projection as new
  mail
- the watcher/reconcile boundary minimum method set is defined in product
  [architecture.md §21.6.1](../architecture.md)

## 3.0.1 Allowed Operating-System Difference Inventory

The Phase S production target allows OS-specific implementation differences
only in these daemon-owned areas:

1. Same-host local IPC transport
   - Unix: Unix domain socket
   - Windows: named-pipe-backed local IPC
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
- replay, retry, and timeout semantics
- watch/reconcile and notification runtime coordination
- shutdown ordering and typed error surfaces

If a code path needs additional platform branching outside the three areas
above, the architecture docs and boundary inventory must be updated before the
implementation is accepted.

## 3.0.2 Shared Frame Contract

The daemon host shell must use the shared ATM frame contract defined in
[`protocol-icd.md`](./protocol-icd.md).

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
- the implementation may use typestate or one internal state enum, but the
  legal lifecycle transitions must remain explicit rather than inferred from
  loosely-coupled booleans
- accepted transition graph:
  - `Starting -> Running`
  - `Starting -> Stopped` on failed startup/rollback
  - `Running -> Draining`
  - `Draining -> Stopped`
- illegal transitions such as `Running -> Starting` or `Stopped -> Running`
  without reinitialization must be prevented by the runtime boundary
- `RuntimeComposition::start()` is the only legal daemon bootstrap entrypoint;
  `run_daemon()` must not bypass the lifecycle root and call the listener
  directly
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
- watcher/reconcile submodules expose only the owned watch/reconcile boundary;
  debounce state, scan cursors, and filesystem adapter details remain
  crate-private
- plugin/notifier submodules expose only notifier/plugin boundary traits or
  façades required by runtime composition; delivery internals remain
  crate-private
- observability submodules expose only the daemon-owned event sink façade used
  by runtime composition; sink plumbing and field-shaping helpers remain
  crate-private

Transport dispatcher rule:
- local-IPC and TCP/TLS listener/connection receive loops are deliberately tiny
- they may:
  - read a framed request
  - parse a qualified request type
  - dispatch through one injected dispatcher boundary
  - encode a typed response
- they may not:
  - run SQL directly
  - invoke watcher reconciliation directly
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

Required daemon-private partitions:
- `ownership`
  - owns host-wide lock paths, owner-record reads/writes, stale-owner recovery,
    and singleton cleanup rules
- `server_runtime`
  - owns listener bootstrap, accept loop, connection registry, drain
    sequencing, and forced-cancel escalation
- `request_runtime`
  - owns per-connection request execution, request-work tracking, request
    deadlines, and response emission
- `runtime_status`
  - owns the live status cache, cache-cap semantics, roster hydration,
    reload-time runtime-view assembly, and doctor-health projection into
    `atm doctor`
- `peer_transport`
  - owns remote delivery, replay, retry, and remote transport-specific failure
    handling
- `watch_runtime`
  - owns bounded watch subscription state and watch worker polling
- `reconcile_runtime`
  - owns reconcile debounce, coalescing, and bounded pending-work wakeups
- `notification_runtime`
  - owns bounded notification delivery worker state and notifier wakeups

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
7. clear or invalidate current owner metadata while `owner.lock` is still held
8. release the live exclusive ownership lock
9. remove the same-host listener artifact if the local-IPC adapter requires a
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
- bounded remote retry queue depth: `256`
- SQLite handle/pool budget: min `1`, max `4`
- live status-cache cap: `4096` entries
- reconcile notification fingerprint registry cap: `1024` keys
- watch subscription cap: `256` active subscriptions
- notification work queue depth: `256`

Required saturation behavior:
- connection cap exceeded: reject new accepts with a typed over-capacity error
- per-connection inflight exceeded: reject excess requests on that connection;
  in Phase R the transport remains single-request-per-connection, so the
  in-flight count is structurally `1` until framed multiplexing exists
- ingest queue full: fail the enqueue with structured degradation/health
  reporting; no silent drop
- retry queue full: fail remote send attempt rather than enqueueing unbounded
- watch subscription cap exceeded: reject the new subscription with typed
  over-capacity failure rather than retaining unbounded watcher state
- reconcile notification fingerprint registry cap exceeded: evict the oldest
  tracked key before inserting the new key so the daemon preserves the latest
  active reconcile targets without retaining unbounded fingerprint state
- notification queue full: fail the enqueue with typed degraded delivery status
  rather than silently buffering beyond the cap
- status-cache cap exceeded: evict least-recently-updated noncritical entries
  from the live-member map so the retained map cardinality remains bounded;
  removed entries project as explicit `unknown` on later snapshot/doctor reads
  and still emit structured warning output

## 3.3 Status Ownership

The daemon owns the live runtime view of agent status.

Architectural rules:
- live status remains in daemon memory
- current agent `pid` is durable SQLite truth and is cached in daemon memory
  as the primary liveness field
- `last_active_at` remains in daemon memory alongside live status
- daemon-managed team-member fields update only through the documented heartbeat
  socket handler in `docs/team-member-state.md`
- SQLite does not own live status or `last_active_at`; it owns durable roster
  state and the current per-member `pid`
- status cache rebuild after restart hydrates configured roster members as
  `unknown`, consults durable SQLite pid continuity only as startup fallback,
  and refreshes thereafter through runtime events
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
- per-leg TCP/TLS connect deadline: `5s`
- per-leg TCP/TLS read/write deadline: `5s`
- total remote retry budget default: `30s` via
  `daemon.remote_retry_budget`
- SQLite `busy_timeout`: `5000ms`
- ingest batch processing slice: `2s` max before yielding
- daemon health query used by `atm doctor`: `3s`
- lifecycle wake-worker join during runtime teardown: `1s` max
- reconcile runtime drain during runtime teardown: `2s` max
- watch runtime drain during runtime teardown: `2s` max
- retained-log flush and sync during runtime teardown: `2s` best-effort max

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
- readiness states are:
  - `ready` when the daemon owns the runtime, SQLite-backed continuity is
    available, ingest is healthy, and no active identity-conflict path exists
  - `degraded` when the daemon is still running but SQLite continuity, ingest,
    or identity-conflict handling is impaired
  - `unavailable` when the daemon still owns the runtime but every tracked
    member has transitioned fully offline
- the runtime health snapshot projected into `atm doctor` must also carry:
  - singleton-owner pid when known
  - SQLite-ready state
  - degraded-ingest state
  - aggregate active/idle/offline/unknown member counts

## 3.6 Crash Recovery

Crash recovery must preserve durable truth and compatibility export ordering.

Required rules:
- durable ordering is `SQLite commit -> Claude export / remote handoff`
- export/re-export must be keyed by durable `message_key`
- if a crash occurs after SQLite commit but before export completes, recovery
  must resume from durable state keyed by `message_key`
- bounded retry/re-export state required after daemon crash must be stored in
  SQLite with an expiry/deadline, not only in RAM
- WAL checkpoint is attempted on graceful shutdown, but crash recovery must not
  depend on graceful shutdown having completed
- recovery must not turn bounded transient retry state into a long-lived
  durable remote outbox; expired retry rows are purged/fail closed on replay

## 4. ADR Namespace

The `atm-daemon` crate uses the `ADR-DAEMON-*` namespace.

Initial use cases:

- singleton runtime enforcement
- local transport adapter structure
- remote daemon-to-daemon protocol structure
- runtime watch/reconcile orchestration
- queued notifier/runtime delivery structure
  - bounded at `256` in-memory events with typed backpressure on overflow
