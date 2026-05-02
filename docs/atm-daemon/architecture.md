# ATM-Daemon Crate Architecture

## 1. Purpose

This document defines the `atm-daemon` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns runtime composition only.

This crate is introduced by the Phase Q implementation line and is not part of
the current pre-Phase-Q workspace yet.

## 2. Responsibilities

The `atm-daemon` crate is responsible for:

- singleton daemon startup and ownership checks
- local daemon API listener
- remote daemon-to-daemon transport listener/client
- runtime wiring of `atm-core` service boundaries
- live agent-status cache
- optional watch/reconcile runtime loop
- daemon/runtime observability emission
- daemon health/status query surface for `atm doctor`

The `atm-daemon` crate must remain thin.

## 3. Architectural Rules

- `atm-daemon` must not reimplement `atm-core` business logic.
- `atm-daemon` must not access SQLite except through the `atm-core` store
  boundary.
- `atm-daemon` must not parse or write inbox JSONL except through the
  `atm-core` ingress/export boundaries.
- `atm-daemon` owns one protocol with multiple transport implementations:
  - Unix domain socket
  - TCP/TLS
  - in-process `test-socket`
- cross-host delivery is daemon-to-daemon only.
- remote delivery may use bounded transient retry, but not a durable long-lived
  remote outbox.
- remote send success is defined by remote daemon acceptance within the bounded
  retry window.
- daemon runtime failures must remain typed and must not depend on
  panic/unwrap for routine transport, socket, or store-boundary failure.
- daemon observability remains structured through `sc-observability`; no ad hoc
  debug-only runtime path replaces it in production.
- plugin-local observability does not replace daemon-owned runtime/transport
  sinks; daemon-owned events stay daemon-owned.
- runtime subsystems stay fully isolated:
  - SQL/store calls belong only to the store boundary
  - file-watch/reconcile logic belongs only to the watcher/reconcile boundary
  - notification delivery belongs only to the notifier/plugin boundary
  - socket I/O belongs only to the transport boundary
- watcher/reconcile adapters remain crate-private and dispatch through owned
  ingress/service handlers rather than touching store/transport/notifier
  internals directly

## 3.1 Singleton Runtime

Hard invariant:
- it must be impossible for two active ATM daemons to run on one host at the
  same time

Architectural rule:
- singleton enforcement belongs in the runtime wrapper only
- the runtime must fail closed rather than allowing split ownership

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

Privacy boundary:
- the lifecycle state type and transport/runtime adapter internals remain
  crate-private implementation details
- public callers interact through daemon request/response surfaces and health
  queries, not through direct state mutation
- transport submodules expose only the listener/client boundary types required
  for runtime composition; frame codecs, connection state, and socket helpers
  remain crate-private
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

Socket dispatcher rule:
- listener/connection receive loops are deliberately tiny
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
  transport so handler behavior is testable without Unix/TCP socket code

Dispatcher/handler rule:
- request-kind routing belongs to the dispatcher boundary, not to the socket
  adapter
- concrete request-family behavior belongs to injectable handlers behind that
  dispatcher
- Unix domain socket and TCP/TLS adapters share the same dispatcher/handler
  contract
- the dispatcher itself stays thin and must not absorb request-family business
  logic

## 3.1.1 Graceful Shutdown

Shutdown is part of the daemon contract, not an implementation detail.

Required shutdown sequence:
1. stop accepting new local and remote connections
2. mark the runtime as draining so new work fails clearly
3. allow inflight work to finish within the drain deadline
4. cancel remaining inflight work at the force-cancel deadline
5. checkpoint SQLite WAL
6. flush observability sinks on a best-effort basis
7. release singleton socket/ownership artifacts

Required deadlines:
- normal drain deadline: `5s`
- force-cancel deadline after drain starts: `10s` total

Ordering rule:
- singleton ownership is released only after listener shutdown and checkpoint
  sequencing completes or the runtime has failed closed

## 3.1.2 Signal Handling

Required signals:
- `SIGINT`: begin graceful shutdown
- `SIGTERM`: begin graceful shutdown
- `SIGHUP`: trigger bounded configuration / roster rescan without dropping
  singleton ownership

Architectural rules:
- signal handlers install before any listener begins accepting
- signal-triggered shutdown uses the same drain/checkpoint/release path as an
  explicit runtime stop
- singleton ownership artifacts must be released on normal signal-driven exit
  and retained only on crash/fail-stop paths where the process cannot run
  cleanup code

## 3.2 Status Ownership

The daemon owns the live runtime view of agent status.

Architectural rules:
- live status remains in daemon memory
- SQLite may retain a diagnostic snapshot only
- status cache rebuild after restart begins from `unknown` and refreshes through
  runtime events

## 3.2.1 Resource Caps And Saturation

The daemon must use explicit, small resource ceilings.

Required caps:
- max concurrent accepted connections: `64`
- max per-connection inflight requests: `32`
- ingest queue depth: `1024`
- bounded remote retry queue depth: `256`
- SQLite handle/pool budget: min `1`, max `4`
- live status-cache cap: `4096` entries

Required saturation behavior:
- connection cap exceeded: reject new accepts with a typed over-capacity error
- per-connection inflight exceeded: reject excess requests on that connection
- ingest queue full: fail the enqueue with structured degradation/health
  reporting; no silent drop
- retry queue full: fail remote send attempt rather than enqueueing unbounded
- status-cache cap exceeded: evict least-recently-updated noncritical entries
  to `unknown` with structured warning emission

## 3.2.2 Timeouts

Required timeout defaults:
- same-host daemon request deadline: `3s`
- per-leg TCP/TLS connect deadline: `5s`
- per-leg TCP/TLS read/write deadline: `5s`
- total remote retry budget: `30s`
- SQLite `busy_timeout`: `1500ms`
- ingest batch processing slice: `2s` max before yielding
- daemon health query used by `atm doctor`: `3s`

## 3.3 Test Strategy

The daemon is not the core test strategy.

Architectural rules:
- `atm-daemon` should be testable primarily through in-process harnesses and
  fakes around its adapters
- if process-level daemon smoke tests exist, they must remain small and
  separate
- no core ATM correctness rule should require a real daemon process for normal
  validation
- `atm doctor` and other daemon-querying CLI flows must rely on explicit daemon
  request/response paths, not private inspection shortcuts

## 3.4 Crash Recovery

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
