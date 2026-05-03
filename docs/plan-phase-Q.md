# Phase Q Plan

Branch: `plan/phase-Q`
Base: `develop` (`9d3bd4d`)

## Goal

Replace the filesystem JSON mailbox store as ATM's mail source of truth with
SQLite, while keeping the Claude inbox path as the required delivery and
context-injection surface for Claude clients.

## Motivation

Phase P improved the current file-based model enough for interim release use,
but `docs/lock-release-gate.md` concluded that the mailbox-lock architecture
still has bounded but real failure modes under contention and crash recovery.
Phase Q removes mailbox correctness from that lock model instead of hardening
it further.

Key consequences from the gate:
- file locks may remain as transitional compatibility mechanisms only
- ATM command correctness must stop depending on `.lock` sentinel cleanup
- the long-term answer is one ATM-owned transactional store

## Architecture Decision

### Decision

Use one SQLite database per team in WAL mode as the single source of truth for
ATM mail state.

Proposed path:
- `.claude/teams/<team>/.atm-state/mail.db`

SQLite mode and invariants:
- `journal_mode = WAL`
- `foreign_keys = ON`
- all ATM mutating commands use explicit transactions
- mailbox `.json` files stop being ATM's source of truth

Roster and status split:
- SQLite is the source of truth for team roster
- live agent status is daemon runtime state, not SQLite truth
- SQLite may persist last observed status for diagnostics only, but that
  snapshot is not authoritative live state

### What Remains On Filesystem

Claude inbox JSONL files remain required for:
- Claude context injection
- interoperability with direct Claude-native `SendMessage` producers
- append-only delivery/export performed by ATM `send` and ack replies

Those files become:
- external ingress for Claude-authored messages
- compatibility/export surface for ATM-authored messages
- not the authoritative store for ATM read/ack/clear/task state

## Information Flow

Phase Q must make the data flow explicit.

### Claude / Compatibility Path

- Claude `SendMessage` and other legacy writers append JSONL to inbox files
- ATM imports those records into SQLite through one owned ingress boundary
- ATM `send` and ack replies export Claude-compatible JSONL records when the
  recipient requires Claude context injection

Rules:
- JSONL remains mandatory for Claude compatibility
- JSONL is not the authoritative durable store for ATM mail semantics
- ATM-only fields remain under `metadata.atm`

### Native Agent Path

- native agent/plugin traffic must not use JSONL
- native agent processes communicate with ATM over one daemon API
- those messages commit to SQLite directly through the owned store boundary

### Remote Host Path

- remote delivery is daemon-to-daemon only
- agent/plugin code never talks cross-host directly
- addressing expands from `agent@team` to `agent@team.host`

Rules:
- local routing and remote routing use the same logical API
- only the transport adapter changes
- sender-side daemons do not write remote host inbox JSONL directly

### Command Model

- `send`
  - insert authoritative message/state rows in SQLite
  - append Claude-compatible export record to recipient inbox
- `read`
  - ingest unseen inbox rows into SQLite
  - read from SQLite projection
- `ack`
  - ingest before acting
  - update authoritative ack/task state in SQLite
  - append reply export record when required
- `clear`
  - ingest before acting
  - mark hidden/cleared in SQLite
  - no correctness-critical inbox rewrite

## Runtime And Transport Model

Phase Q needs one logical interface and multiple transport adapters.

### One Interface

There must be one logical daemon API for:
- message delivery
- acknowledgement
- live status updates
- team/roster queries if needed
- notification delivery into native agent processes

### Two Transport Implementations

- same-host transport: Unix domain socket
- cross-host transport: TCP/TLS
- test transport: in-process `test-socket`

Rules:
- one protocol/interface
- multiple transport implementations
- not two separate systems
- `test-socket` uses the same dispatcher/handler contract so boundary tests do
  not depend on real socket implementations

### Daemon-To-Daemon Remote Delivery

Cross-host communication happens only between two daemons.

Flow:
1. local client/plugin talks to local daemon
2. local daemon decides local vs remote route
3. remote sends go daemon-to-daemon over TCP/TLS
4. receiving daemon commits locally, then exports/notifies locally

Remote host failure semantics:
- no durable long-term remote outbox
- bounded transient retry is allowed only for short intermittent failures
- if the remote host remains unreachable after the bounded retry window,
  `send` fails and the message is not left queued for days
- a remote send is not considered successful until the remote daemon accepts it

## Daemon Model

The daemon is part of the target runtime, but it must remain a thin runtime
wrapper rather than the place where business logic lives.

Hard requirements:
- exactly one daemon per host
- it must be impossible for two active ATM daemons to run on one host at the
  same time
- daemon startup must fail deterministically when a live daemon already owns
  the host runtime
- stale daemon ownership artifacts may be cleaned up only when they are proven
  stale; stale cleanup must never allow two live daemons

Runtime responsibility:
- socket listeners
- local/remote transport adapters
- filesystem watch/reconcile runtime if enabled
- live agent status cache

Non-responsibility:
- daemon must not own unique business logic that is unavailable to in-process
  service callers
- daemon unavailability must surface as an explicit runtime failure, not as
  hidden direct SQLite/JSONL fallback; the documented daemon auto-start path is
  the only allowed startup behavior

## Plugin Model

Native agent/plugin integration is a separate path from Claude JSONL.

Rules:
- plugin traffic must not use JSONL
- plugin code talks to the local daemon only
- cross-host delivery remains daemon-to-daemon
- the later agent plugin crate must be started early enough that its runtime
  interface is not overlooked during the SQLite migration

Minimum plugin expectations:
- receive message/task notifications from the local daemon
- report live status to the local daemon
- later support direct send/ack operations through the daemon API

## Strict I/O Boundary Rule

This is a top-level architecture requirement for Phase Q:

- every subsystem is behind a strict trait boundary for all I/O

Meaning:
- only the store subsystem may touch SQLite
- only the inbox ingress/export subsystem may parse or write inbox JSONL
- only the config ingress subsystem may parse team `config.json`
- only the watcher/reconcile subsystem may consume watch events or drive
  reconcile scheduling
- only the transport subsystem may touch sockets
- only the notifier/plugin subsystem may talk to agent processes

No exceptions:
- no direct filesystem access outside the owning subsystem
- no direct SQLite access outside the owning subsystem
- no direct socket access outside the owning subsystem
- no business logic in adapter code
- no public concrete adapter internals when a private implementation behind a
  trait/façade boundary can enforce the same behavior

Enforcement approach:
- prefer strict module privacy and hidden concrete implementations even before
  crate extraction
- use one trait boundary per I/O-owning subsystem
- if a boundary proves fragile, extract it into a separate crate later

## Observability And Error Model

Phase Q must keep observability and failure handling structured.

Rules:
- CLI and daemon both emit structured events through `sc-observability`
- `atm-core` owns ATM event and error models; adapters emit them through the
  shared observability boundary
- fallible runtime behavior uses typed `Result`/error-enum boundaries rather
  than panic/unwrap as the normal control flow
- production panic is reserved for invariant corruption or unreachable code,
  not routine I/O, parse, or transport failure
- `atm doctor` remains a CLI command, but in the Phase Q target runtime it must
  be able to query daemon/runtime state rather than assuming a daemon-free
  environment

Required Phase Q error families:
- store
- ingest
- export
- transport
- daemon runtime
- daemon singleton
- daemon client

Each family must map to concrete `AtmErrorCode` variants before the Phase Q
implementation is considered complete.

## Schema Design

Phase Q should start with a deliberately small schema.

### `messages`

Authoritative logical message row.

Suggested columns:
- `message_key TEXT PRIMARY KEY`
- `team_name TEXT NOT NULL`
- `recipient_agent TEXT NOT NULL`
- `sender_display TEXT NOT NULL`
- `sender_canonical TEXT NULL`
- `sender_team TEXT NULL`
- `body TEXT NOT NULL`
- `summary TEXT NULL`
- `created_at TEXT NOT NULL`
- `source_kind TEXT NOT NULL`
- `legacy_message_id TEXT NULL`
- `atm_message_id TEXT NULL`
- `raw_metadata_json TEXT NULL`

Rules:
- `message_key` is the canonical ATM identity key
- preferred forms:
  - `atm:<ulid>`
  - `legacy:<uuid>`
  - `ext:<sha256>` for external Claude-native messages with no ATM id
- `legacy_message_id` and `atm_message_id` stay unique when present

### `inbox_ingest`

Tracks imported filesystem records so external inbox writes become durable in
SQLite without duplicate imports.

Suggested columns:
- `team_name TEXT NOT NULL`
- `recipient_agent TEXT NOT NULL`
- `source_path TEXT NOT NULL`
- `source_fingerprint TEXT NOT NULL`
- `message_key TEXT NOT NULL`
- `imported_at TEXT NOT NULL`

Primary key:
- `(team_name, recipient_agent, source_fingerprint)`

Rules:
- `source_fingerprint` prefers stable ids when present
- fallback is a deterministic hash of mailbox identity plus canonicalized raw
  record for external messages without ATM ids

### `ack_state`

Authoritative acknowledgement state.

Suggested columns:
- `message_key TEXT PRIMARY KEY`
- `pending_ack_at TEXT NULL`
- `acknowledged_at TEXT NULL`
- `ack_reply_message_key TEXT NULL`
- `ack_reply_team TEXT NULL`
- `ack_reply_agent TEXT NULL`

Rules:
- one row per ack-capable logical message
- absence of row means no ATM ack semantics

### `tasks`

Basic task table for task-linked mail.

Suggested columns:
- `task_id TEXT PRIMARY KEY`
- `message_key TEXT NOT NULL`
- `status TEXT NOT NULL`
- `created_at TEXT NOT NULL`
- `acknowledged_at TEXT NULL`
- `metadata_json TEXT NULL`

Rules:
- initial statuses can stay minimal:
  - `pending_ack`
  - `acknowledged`

### `team_roster`

Authoritative team roster state.

Suggested columns:
- `team_name TEXT NOT NULL`
- `agent_name TEXT NOT NULL`
- `role TEXT NULL`
- `transport_kind TEXT NULL`
- `host_name TEXT NULL`
- `pid INTEGER NOT NULL`
- `metadata_json TEXT NULL`

Primary key:
- `(team_name, agent_name)`

Rules:
- roster truth lives in SQLite
- current per-member `pid` lives in SQLite as durable truth and is cached by
  the daemon as its primary liveness field
- `last_active_at` does not live in `team_roster`; it is daemon-memory-only
  runtime state
- `config.json` becomes an ingress/update source, not the source of truth

### `message_visibility`

ATM-owned display state.

Suggested columns:
- `message_key TEXT PRIMARY KEY`
- `read_at TEXT NULL`
- `cleared_at TEXT NULL`

Rules:
- read/unread and clear state stop rewriting source inbox rows for correctness

## Migration Strategy

Recommended strategy: staged cutover, not big-bang replacement.

Important runtime note:
- do not make daemon-first runtime work the foundation of the migration
- first prove the strict service boundaries and SQLite ownership in-process
- add the daemon runtime only as a thin wrapper over those proven boundaries

### Stage 1: Introduce Store Boundary

- add one `mail_store` owner boundary in `atm-core`
- keep existing file-backed behavior behind current code paths
- add SQLite implementation and schema bootstrap
- add the explicit I/O trait boundaries needed for store, inbox ingress/export,
  config ingress, transport, and notification
- define crash-recovery ordering and durable replay state with the schema,
  rather than deferring those rules to the final lock-retirement sprint
- no user-visible command behavior change yet

### Stage 2: Ingest + Dual Write

- ingest mailbox JSONL into SQLite on command entry
- ingest team `config.json` into SQLite roster state
- import existing workflow sidecar state into SQLite during first-run migration
  so ack/read/clear semantics do not regress during cutover
- `send` and ack replies write to SQLite first, then export to inbox
- keep existing read behavior available for comparison/debug only
- SQLite becomes authoritative for new ATM-authored rows

### Stage 3: Read/Ack/Clear Cutover

- `read`, `ack`, and `clear` operate from SQLite after inbox ingest
- stop correctness-critical full-file inbox rewrites
- keep export-only inbox append for Claude delivery

### Stage 4: Thin Daemon Runtime

- add the singleton daemon runtime only after the service boundary is proven
- implement one daemon API with two production transport adapters plus one
  in-process `test-socket`:
  - Unix domain socket
  - TCP/TLS
- keep live status in daemon memory
- keep cross-host routing daemon-to-daemon only

### Stage 5: Compatibility Cleanup

- remove mailbox-lock dependence from runtime correctness
- retire stale-lock cron sweep for mail flows
- keep only any compatibility code still required for non-mail paths

## Backward Compatibility

Phase Q must preserve:
- existing `atm send`, `atm read`, `atm ack`, and `atm clear` CLI contracts
- Claude-native inbox top-level schema
- `metadata.atm` placement for ATM machine fields

Compatibility rules:
- old inbox rows with top-level ATM legacy fields remain readable
- existing workflow sidecar data can be imported during first-run migration
- external Claude-native messages with no ATM ids must still appear in `atm read`
- CLI behavior remains stable even though the daemon becomes the runtime owner
  of durable state changes in the full target architecture

## Sprint Breakdown

### Q.1 — SQLite Store Foundation

Scope:
- add the store boundary family:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
- keep room for later optional boundaries such as `OrchestrationStore`
- add SQLite bootstrap, migrations, and schema
- add transaction helpers
- add explicit watcher/reconcile boundary alongside store/ingress/export
- add explicit dispatcher boundary alongside transport/handlers

Parallelization rule:
- Q.1 defines the lock-in point for the core boundary traits, request/response
  contracts, typed errors, dispatcher/handler seams, and store-family split
- transport, watcher/reconcile, and command-migration work must not proceed as
  parallel implementation slices until those contracts are stable
- after the Q.1 contracts are stable, later work should be split in parallel
  against those boundaries rather than serializing all implementation in one
  stream

Expected files / crates:
- `crates/atm-core/src/mail_store/*`
- `crates/atm-core/src/task_store/*` or equivalent
- `crates/atm-core/src/roster_store/*` or equivalent
- `crates/atm-rusqlite/src/*`
- `crates/atm-core/src/service/*` or equivalent service boundary module
- `crates/atm-daemon/*` not yet required beyond placeholder boundary docs

Implementation details:
- keep SQL hidden behind the `atm-rusqlite` crate
- keep business logic depending on abstract `MailStore`, `TaskStore`, and
  `RosterStore` traits defined in `atm-core`
- keep schema bootstrap/migrations centralized
- define the canonical `message_key` model here before command cutover begins
- define typed error enums for store/bootstrap failures before command cutover
  spreads ad hoc error translation
- define crash-recovery durable state now:
  - ordering rule `SQLite commit -> export`
  - re-export keyed by `message_key`
  - bounded retry/replay state durable in SQLite with expiry

Acceptance:
- database opens under `.atm-state/mail.db`
- schema bootstrap is deterministic and idempotent
- store-layer tests cover create/read/update transaction basics
- only `atm-rusqlite` owns direct SQLite calls in the first implementation
- `MailStore` is not used as the long-term owner of task-only or roster-only
  domains
- watcher/reconcile logic is isolated behind its own boundary and does not
  bypass ingress/store/notifier ownership
- transport-boundary tests can use `test-socket` without changing handler
  business logic
- the boundary traits and request/result contracts are stable enough to allow
  parallel follow-on implementation without transport or handler drift
- the dispatcher/handler contract is explicit enough that Unix, TCP/TLS, and
  `test-socket` adapters can evolve without absorbing request-family logic
- tests cover:
  - repeated bootstrap
  - transactional rollback on mid-operation failure
  - uniqueness of `message_key`, `legacy_message_id`, and `atm_message_id`
  - roster row replacement/update behavior
  - structured store/bootstrap errors remain discriminated and do not panic on
    routine failure

### Q.2 — Inbox Ingest + Send Dual Write

Scope:
- ingest external inbox rows into SQLite
- ingest existing workflow sidecar state into SQLite
- ingest `config.json` roster updates into SQLite
- move ATM `send` to SQLite-first plus inbox export
- keep exported envelope Claude-native + `metadata.atm`

Expected files / crates:
- `crates/atm-core/src/inbox_ingress/*`
- `crates/atm-core/src/inbox_export/*`
- `crates/atm-core/src/team_ingress/*`
- `crates/atm-core/src/send/*`

Implementation details:
- inbox import must be idempotent
- import must support:
  - `metadata.atm.messageId`
  - legacy top-level `message_id`
  - deterministic fallback fingerprint for external messages without ATM ids
- malformed external records must degrade safely rather than corrupting SQLite
- workflow sidecar import must preserve current ack/read/clear semantics for
  already-known messages
- once roster truth is authoritative in SQLite, the send path must include the
  authoritative `recipient_pane_id` in `ATM_POST_SEND` for post-send hooks when
  that pane mapping is known
- post-send hook implementations should consume `ATM_POST_SEND.recipient_pane_id`
  instead of rediscovering pane mappings from local files once this field is
  available
- ingest/export paths must emit structured `sc-observability` events for import
  success, degradation, and export failure
- after Q.1, this slice can proceed in parallel with transport and
  watcher/reconcile implementation because it depends only on the locked
  ingress/store/export contracts

Acceptance:
- `send` inserts authoritative rows in SQLite
- ATM-authored inbox export still works for Claude recipients
- repeated ingest does not duplicate imported records
- tests cover:
  - duplicate import suppression for ATM-authored and Claude-native rows
  - malformed JSONL record handling without panic
  - malformed `metadata.atm` handling without message loss when Claude fields are usable
  - partial workflow-sidecar import / missing sidecar rows
  - `config.json` roster changes updating SQLite roster truth deterministically
  - `ATM_POST_SEND.recipient_pane_id` populated from roster truth when the
    recipient pane mapping is known
  - structured ingest/export error variants and observability events for the
    degraded paths above

### Q.3 — Ack/Task Migration

Scope:
- move ack state and task state to SQLite
- append reply exports after SQLite commit
- stop treating inbox mutation as authoritative ack state

Expected files / crates:
- `crates/atm-core/src/ack/*`
- `crates/atm-core/src/tasks/*` or equivalent
- `crates/atm-core/src/workflow/*` migration helpers

Implementation details:
- reply export happens only after SQLite commit succeeds
- ack/task transitions must not require rewriting the source inbox record
- existing workflow sidecar state is read only for migration/backfill once
  SQLite is authoritative
- ack/task failure modes must remain typed across service and export boundaries
- after Q.1, this slice can proceed in parallel with transport/runtime work so
  long as it stays within the locked store/export/handler contracts

Acceptance:
- ack-required messages are authoritative in SQLite
- task linkage and acknowledged state survive restart without inbox rewrites
- reply export still lands in Claude inbox correctly
- tests cover:
  - ack-required imported legacy message
  - task-linked imported message
  - reply export failure after commit surfaces clearly and does not corrupt SQLite
  - duplicate ack attempt rejection against SQLite truth
  - no ack/task runtime failure path relies on panic/unwrap

### Q.4 — Read/Clear Cutover + Thin Daemon Runtime

Scope:
- `read` projects from SQLite after ingest
- `clear` updates SQLite visibility state
- remove correctness-critical full-file mailbox rewrites from these paths

Expected files / crates:
- `crates/atm-core/src/read/*`
- `crates/atm-core/src/clear/*`
- projection helpers in `crates/atm-core/src/service/*` or equivalent
- `crates/atm-daemon/src/*`
- `crates/atm/src/*` daemon client wiring

Implementation details:
- `read` must reconcile new external inbox writes before projection
- `clear` becomes SQLite visibility mutation, not inbox truth mutation
- imported legacy messages and forward ATM messages must project consistently
- `atm-daemon` owns singleton enforcement and transport only
- local transport uses Unix domain socket
- remote transport uses TCP/TLS daemon-to-daemon only
- transport-boundary tests use the in-process `test-socket`
- live status cache is daemon-memory truth
- daemon emits structured runtime and transport events through
  `sc-observability`
- daemon graceful shutdown sequence:
  - stop accepts
  - drain inflight work for `5s`
  - force-cancel remaining inflight by `10s`
  - checkpoint WAL
  - release singleton artifacts
- signal handling:
  - install before listen
  - `SIGINT`/`SIGTERM` trigger graceful shutdown
  - `SIGHUP` triggers bounded reload/rescan
- timeout defaults:
  - same-host daemon request `3s`
  - TCP/TLS connect `5s`
  - TCP/TLS read/write `5s`
  - remote retry budget `30s`
  - SQLite `busy_timeout` `1500ms`
  - ingest batch slice `2s`
  - doctor query `3s`
- resource caps:
  - max accepts `64`
  - max per-connection inflight `32`
  - ingest queue `1024`
  - retry queue `256`
  - SQLite handles `1..=4`
  - status cache `4096`
- remote delivery success is defined by remote daemon acceptance within the
  bounded retry window
- daemon-unavailable client calls must attempt the documented auto-start once,
  then fail clearly without hidden fallback if the daemon still cannot run
- `atm doctor` must start consuming daemon/runtime state through the same
  request/response boundaries used by production
- after Q.1, Unix transport, TCP/TLS transport, `test-socket`,
  watcher/reconcile, and daemon-query plumbing should be split into parallel
  implementation slices against the shared dispatcher/handler contracts

Acceptance:
- `read` and `clear` no longer require mailbox rewrite correctness
- lock contention on inbox files does not block SQLite-owned state transitions
- existing CLI output remains compatible
- tests cover:
  - mixed imported legacy + forward ATM rows
  - repeated `read` after external Claude append
  - clear of ack-pending message remains forbidden
  - clear of already-cleared message is idempotent
  - second daemon startup fails deterministically
  - stale singleton artifact cleanup does not allow double-start
  - local same-host daemon API flow
  - local handler flow through `test-socket`
  - bounded remote host unreachable behavior
  - remote acceptance required for send success
  - daemon-unavailable path returns typed error with recovery guidance
  - `atm doctor` can surface daemon/runtime availability without direct socket
    or SQLite bypasses in CLI code

### Q.5 — Lock Retirement + Ops Cleanup

Scope:
- remove mail-flow dependence on mailbox lock cron sweep
- update doctor/restore/backup docs and tooling
- remove or quarantine obsolete mailbox-lock behaviors for mail state

Expected files / crates:
- `crates/atm-core/src/doctor/*`
- `crates/atm-core/src/team_admin/*`

Implementation details:
- mailbox locks may remain only for transitional compatibility paths outside
  normal mail correctness
- no single transport/runtime file should become a catch-all business-logic
  class; keep adapters thin and private behind the protocol boundary
- doctor/restore/backup documentation must describe SQLite + daemon ownership
  rather than the old file-truth lock model
- remaining compatibility-only lock logic must be diagnosable but non-blocking
  for normal mail correctness

Acceptance:
- mail flows do not require the 5-minute stale-lock sweep
- operational docs match SQLite ownership
- Phase Q release gate proves normal mail operation without mailbox-lock
  correctness dependence
- tests cover:
  - stale lock artifacts no longer wedge normal mail flows
  - doctor/restore paths behave correctly with SQLite-owned durable state
  - compatibility-only remaining lock artifacts surface as diagnostics rather
    than correctness blockers
  - no core test requires daemon process spawning

## Testing Constraints

Phase Q must explicitly avoid the daemon failures that sank the earlier design.

Hard requirements:
- daemon spawning is not part of the core test strategy
- core service behavior must be testable without spawning a daemon process
- watcher, transport, and runtime logic must be testable with in-process
  fakes/harnesses
- if any process-level daemon smoke tests exist, they must be tiny and separate
  from ordinary default test runs

Non-negotiable constraint:
- no test architecture may depend on daemon process spawning to validate core
  mail correctness

## Risk Register

### 1. External Claude Messages Bypass SQLite

Risk:
- Claude-native producers write directly to inbox JSONL

Mitigation:
- deterministic command-path ingest before `read`/`ack`/`clear`
- keep ingest idempotent and bounded

### 2. Historical Messages Without Stable ATM Identity

Risk:
- legacy Claude-native rows may lack `message_id` / `metadata.atm.messageId`

Mitigation:
- explicit fallback fingerprint for import
- isolate this in `inbox_ingest`; do not spread ad hoc identity logic

### 3. Export Compatibility Regression

Risk:
- SQLite migration breaks Claude context injection

Mitigation:
- retain raw inbox export tests
- validate exported records against Claude-native schema plus `metadata.atm`

### 4. Mixed-Version Operation During Cutover

Risk:
- older commands still expect filesystem truth

Mitigation:
- stage rollout behind clear sprint gates
- do not remove compatibility readers until cutover is complete

### 5. WAL / Backup / Restore Surprises

Risk:
- current team backup/restore flows are file-oriented

Mitigation:
- add explicit SQLite backup/restore handling in Q.5
- test restore with WAL and checkpointed states

### 6. Scope Sprawl

Risk:
- Phase Q turns into a database/platform rewrite

Mitigation:
- keep v1 schema small
- keep daemon runtime thin and late in the phase
- no generalized query engine
- no broad plugin/event redesign inside this phase

### 7. Boundary Leakage

Risk:
- subsystems bypass their owning trait boundaries and recreate the old
  refactor trap

Mitigation:
- make strict I/O ownership a written requirement
- review every I/O path against the owning subsystem boundary
- reject bypasses in QA on every pass

### 8. Daemon Explosion

Risk:
- accidental multi-daemon behavior or daemon-spawn-heavy tests recreate the
  old runaway process problem

Mitigation:
- singleton daemon is a hard requirement
- no daemon-spawn-based core tests
- daemon runtime stays thin enough that most logic is testable in-process

## QA Invariants

The following must be checked on every QA pass for Phase Q:

- it is impossible for two active ATM daemons to run on one host
- every subsystem performs all of its external I/O only through its owning
  trait boundary
- any SQL, watcher, notifier, or socket-boundary bypass is an immediate QA
  failure
- no business logic has leaked into transport/runtime adapter code
- daemon spawning is not part of the core test strategy
- transport/runtime code remains thin and does not collapse into a giant socket
  class
- socket receive loops are tiny dispatcher loops only
- any socket loop that performs SQL, watcher, notifier, or workflow logic is
  an immediate QA failure
- any watcher/reconcile implementation that performs SQL, socket, or notifier
  logic inline is an immediate QA failure
- CLI and daemon both retain structured `sc-observability` coverage
- typed runtime errors remain discriminated across service, daemon, and CLI
  boundaries
- roster truth lives in SQLite; live status truth lives in daemon memory
- Claude compatibility continues to use Claude-native top-level fields plus
  `metadata.atm`

## Release Gate For Phase Q

Phase Q should be considered complete only when:
- ATM mail correctness no longer depends on mailbox `.lock` files
- SQLite is the authoritative store for read/ack/clear/task semantics
- SQLite is the authoritative store for team roster
- live status is owned by the runtime layer rather than being treated as
  durable database truth
- Claude inbox files remain a compatible export/ingest surface only
- stale lock cleanup can no longer wedge normal ATM mail flows
