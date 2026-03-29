# ATM CLI Architecture

## 1. Overview

The rewrite keeps ATM as a file-based mail CLI and removes daemon architecture.

The workspace remains intentionally small:
- `atm-core`: reusable library
- `atm`: CLI binary

The CLI stays thin. Product logic moves into `atm-core`.

The retained command surface is:
- `send`
- `read`
- `log`
- `doctor`

## 2. Crate Boundaries

### 2.1 `atm-core`

`atm-core` owns:
- path and home resolution
- config and Claude settings resolution
- bridge-hostname resolution for origin inbox merge
- hook-based identity resolution
- address parsing
- file policy enforcement for `send --file`
- team config loading
- mailbox read and write logic
- canonical workflow state classification
- legal workflow state transitions
- send and read service functions
- log query/follow service functions over an injected observability port
- local doctor/diagnostic service functions
- structured error types
- observability event/query models and the injected observability boundary

`atm-core` must not depend on clap or terminal formatting concerns.

### 2.2 `atm`

`atm` owns:
- clap argument parsing
- command dispatch
- output rendering
- process exit behavior
- one-time observability initialization
- the concrete `sc-observability` implementation of the observability port
- injection of that implementation into `atm-core` services at startup

`atm` must not implement mailbox, config, workflow, logging-query, or doctor business logic directly.

### 2.3 Shared Observability Boundary

`atm-core` must not import `sc-observability` directly.

Instead, `atm-core` defines a sealed `ObservabilityPort` boundary plus ATM-owned event and query models. `atm` implements that port using `sc-observability`.

ATM still owns:
- ATM-specific event naming
- ATM-specific structured fields
- mapping CLI filters to shared query/follow APIs
- ATM doctor projections over shared health models

`sc-observability` should own as much generic functionality as possible:
- emission
- record storage and retention policy
- historical query
- follow/tail
- severity filtering
- structured field filtering
- runtime health reporting

An early ATM planning/coordination sprint, `OBS-GAP-1`, must verify and close this shared API surface before ATM log/doctor implementation proceeds.

## 3. Module Layout

Planned `atm-core` layout:

```text
crates/atm-core/src/
  lib.rs
  address.rs
  config/
    aliases.rs
    bridge.rs
    discovery.rs
    mod.rs
    types.rs
  doctor/
    health.rs
    mod.rs
    report.rs
  error.rs
  home.rs
  identity/
    hook.rs
    mod.rs
  log/
    filters.rs
    mod.rs
  mailbox/
    atomic.rs
    hash.rs
    lock.rs
    mod.rs
    store.rs
  model_registry.rs
  observability.rs
  read/
    filters.rs
    mod.rs
    seen_state.rs
    state.rs
    wait.rs
  schema/
    agent_member.rs
    inbox_message.rs
    mod.rs
    permissions.rs
    settings.rs
    team_config.rs
  send/
    file_policy.rs
    input.rs
    mod.rs
    summary.rs
  text.rs
  types.rs
```

Planned `atm` layout:

```text
crates/atm/src/
  main.rs
  commands/
    doctor.rs
    log.rs
    mod.rs
    read.rs
    send.rs
  observability.rs
  output.rs
```

Notes:
- no plugin framework
- no daemon client
- no runtime spawning layer
- no separate `tail` command
- no separate `status` command in the initial rewrite

## 4. Core Types

### 4.1 Semantic Newtypes

Per `rust-best-practices`, validated primitives and semantic ids should not remain as raw `String` values across the service boundary.

Required public newtypes:
- `TeamName`
- `AgentName`
- `IdentityName`
- `MessageId`
- `MessageBody`
- `MessageSummary`
- `IsoTimestamp`
- `MailAddress`
- `HomeDir`
- `AbsolutePath`
- `LogFieldKey`
- `LogFieldValue`

These are required to reduce repeated validation and remove stringly typed command paths.

### 4.2 Workflow State And Display Types

Canonical workflow enum:

```rust
pub enum MessageState {
    Unread,
    PendingAck,
    Acknowledged,
    Read,
}
```

Display bucket enum:

```rust
pub enum DisplayBucket {
    Unread,
    PendingAck,
    History,
}
```

Selection enum:

```rust
pub enum ReadSelection {
    Actionable,
    UnreadOnly,
    PendingAckOnly,
    ActionableWithHistory,
    All,
}
```

Marking mode:

```rust
pub enum MarkMode {
    ApplyReadTransitions,
    NoMutation,
}
```

Display mapping is fixed:
- `Unread` -> `DisplayBucket::Unread`
- `PendingAck` -> `DisplayBucket::PendingAck`
- `Acknowledged` -> `DisplayBucket::History`
- `Read` -> `DisplayBucket::History`

### 4.3 Typestate Transition Model

Per `rust-best-practices`, legal workflow transitions should be encoded in the type system inside the core pipeline.

Private marker states:

```rust
pub struct UnreadState;
pub struct PendingAckState;
pub struct AcknowledgedState;
pub struct ReadState;

pub struct StoredMessage<S> {
    // persisted fields + state marker
}

impl StoredMessage<UnreadState> {
    pub fn mark_pending_ack(self, at: IsoTimestamp) -> StoredMessage<PendingAckState>;
}

impl StoredMessage<PendingAckState> {
    pub fn acknowledge(self, at: IsoTimestamp) -> StoredMessage<AcknowledgedState>;
}
```

There is no inverse transition.

`ReadState` remains a canonical classification target for legacy and informational messages, even though the normal retained read path does not create it.

The public `MessageState` enum is for reporting and filtering. The typestate markers enforce legal transitions inside `atm-core`.

### 4.4 Log Query Types

Log query types should remain generic enough to map onto shared `sc-observability` APIs.

Required public types:

```rust
pub enum LogMode {
    Snapshot,
    Tail,
}

pub enum LogLevelFilter {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub struct LogFieldMatch {
    pub key: LogFieldKey,
    pub value: LogFieldValue,
}
```

## 5. Persisted Schema

### 5.1 Team Config

The rewrite reuses the existing team config schema where feasible.

Only a small subset is required by the retained surface:
- team name
- member names
- enough member metadata to preserve round-trips
- bridge remote host configuration needed for origin-file merge

### 5.2 Inbox Message

Persisted fields used by the rewrite:
- `from`
- `source_team`
- `text`
- `timestamp`
- `read`
- `summary`
- `message_id`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`
- unknown fields

Canonical workflow state is derived from persisted fields and not serialized separately.

## 6. Public Service APIs

### 6.1 Send Service

Public entrypoint:

`send::send_mail(request: SendRequest, observability: &dyn ObservabilityPort) -> Result<SendOutcome, AtmError>`

`SendRequest` contains:
- home directory
- current directory
- sender override
- target address input
- team override
- message source
- summary override
- dry-run flag

`SendMessageSource` variants:
- inline text
- stdin text
- file reference

`SendOutcome` contains:
- action
- resolved team
- resolved recipient
- resolved sender
- generated message id
- summary
- rendered message body
- delivery result

The file-reference path may be rewritten through the file policy layer.

Send ordering rules:
- resolve target address, team existence, and agent membership as one address-resolution stage before mailbox path selection
- enter the atomic append boundary before final inbox mutation
- validate message text inside the atomic append boundary
- perform duplicate suppression and final append inside the same atomic append boundary

### 6.2 Read Service

Public entrypoint:

`read::read_mail(query: ReadQuery, observability: &dyn ObservabilityPort) -> Result<ReadOutcome, AtmError>`

`ReadQuery` contains:
- home directory
- current directory
- actor override
- optional target address
- team override
- selection mode
- seen-state mode
- mark mode
- limit
- sender filter
- timestamp filter
- optional timeout

Timeout rule:
- if the requested selection is already non-empty after filtering and selection-mode application, return immediately
- otherwise wait for a newly eligible message until the timeout expires

`ReadOutcome` contains:
- action
- resolved team
- resolved agent
- selection mode
- whether history is collapsed
- whether any mutation was applied
- displayed messages
- bucket counts

`ReadOutcome.bucket_counts` exposes:
- unread
- pending_ack
- history

The CLI JSON output mirrors the current contract:
- `action`
- `messages`
- `count`
- `bucket_counts`
- `history_collapsed`

### 6.3 Observability Boundary

The observability boundary is a sealed `ObservabilityPort` (or equivalent injected interface) defined in `atm-core` and implemented in `atm`.

It is responsible for:
- command lifecycle emission
- log query
- log tail/follow
- observability health projection

`atm-core` owns the ATM-specific event and query vocabulary.

`atm` owns the concrete `sc-observability` integration.

### 6.4 Log Service

Public entrypoints:

- `log::query_logs(query: LogQuery, observability: &dyn ObservabilityPort) -> Result<LogSnapshot, AtmError>`
- `log::tail_logs(query: LogQuery, observability: &dyn ObservabilityPort) -> Result<LogTailSession, AtmError>`

`LogQuery` contains:
- mode
- level filter
- field matches
- time window
- limit

`LogSnapshot` contains:
- resolved query
- snapshot ordering metadata
- returned records

`LogTailSession` is an owning stateful object that yields matching records from the shared observability follow API without exposing a public callback trait.

Ordering rules:
- snapshot queries return newest-first records before CLI output limits are rendered
- tail sessions yield records in follow arrival order

ATM must not parse daemon log files directly in this service.

### 6.5 Doctor Service

Public entrypoint:

`doctor::run_doctor(query: DoctorQuery, observability: &dyn ObservabilityPort) -> Result<DoctorReport, AtmError>`

`DoctorQuery` contains:
- home directory
- current directory
- team override

`DoctorReport` contains:
- summary
- findings
- recommendations
- environment override visibility
- observability health

`DoctorFinding` contains:
- severity
- code
- message
- remediation

The report model should reuse the current doctor command’s severity/finding structure where useful, but local checks replace daemon checks.

## 7. Read Pipeline

The read pipeline stages are:
1. resolve actor and target inbox
2. build the hostname registry for configured origin inboxes
3. load mailbox records from the merged inbox surface
4. classify workflow state
5. apply sender and timestamp filters
6. apply seen-state filter unless selection is `All`
7. map workflow state to display bucket and apply selection mode
8. wait if `timeout` is set and the current selection is empty
9. sort newest-first and apply limit
10. apply legal read transitions for displayed unread messages
11. persist state changes atomically
12. update seen-state when enabled
13. return outcome

This ordering is part of the architecture contract.

## 8. Log Pipeline

The log pipeline stages are:
1. resolve the injected observability port implementation
2. map CLI filters into shared query/follow filters
3. query or follow records through the observability port
4. project ATM-owned record fields for CLI rendering
5. return records to the CLI layer

Shared `sc-observability` should own record storage, filtering, and follow mechanics. ATM should own only ATM-specific query defaults and field projections.

## 9. Doctor Pipeline

The doctor pipeline stages are:
1. resolve config and environment overrides
2. resolve effective team and identity inputs
3. verify local team/mailbox/config paths
4. verify hook identity availability
5. verify observability initialization and health
6. verify observability query readiness for `atm log`
7. assemble findings and recommendations
8. render report

## 10. Mailbox Storage

The mailbox layer owns:
- tolerant reads
- atomic append
- duplicate suppression
- conflict merge
- origin-inbox merge
- atomic workflow-state updates

The mailbox layer does not own selection policy, display buckets, output formatting, log query behavior, or doctor diagnostics.

## 11. Identity And File Policy

### 11.1 Hook Identity

Hook-file identity is retained because it is a current non-daemon convenience path for send/read identity resolution.

Only hook identity resolution is required for the rewrite. Session-resolution paths that exist only to bridge runtime/daemon ambiguity are not required.

### 11.2 File Policy

The current `send --file` behavior is retained:
- inspect Claude settings permissions when available
- if the referenced file is allowed, send a direct file reference
- otherwise copy to ATM share storage and rewrite the message body accordingly

## 12. Observability

`atm-core::observability` defines ATM event/query models plus the sealed `ObservabilityPort` boundary.

`atm` provides the concrete `sc-observability` implementation and injects it into core services.

Initialization:
- `atm` initializes logging once at process startup
- `atm` constructs the concrete observability port after startup initialization
- logging failures degrade to no-op behavior for explicit mail commands

Required ATM event classes:
- command start
- command success
- command failure
- mailbox record skipped

Required ATM event fields:
- command
- team
- actor
- target
- outcome
- error class when applicable
- message count when applicable
- transition count when applicable

For explicit observability consumer commands:
- `atm log` depends on shared query/follow APIs
- `atm doctor` depends on shared health APIs
- failures in those consumer paths are command errors, not silently dropped events

## 13. Error Model

Root public error:

```rust
pub struct AtmError {
    pub kind: AtmErrorKind,
    pub message: String,
    pub recovery: Option<String>,
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
```

Required families:
- config
- address
- identity
- team not found
- agent not found
- mailbox read
- mailbox write
- file policy
- validation
- serialization
- timeout
- observability emit
- observability query
- observability health

Every public error must include:
- a stable class
- human-readable cause
- recovery guidance when the user can act

## 14. Trait Policy

The initial rewrite should avoid public extension traits.

If a trait becomes necessary:
- prefer a sealed trait
- verify object safety before stabilization

## 15. Testing Strategy

`atm-core` tests:
- address parsing
- config precedence
- bridge hostname resolution for merged inbox reads
- settings resolution
- hook identity resolution
- file policy behavior
- team membership validation
- tolerant inbox parsing
- origin-inbox merge
- atomic append behavior
- duplicate suppression
- workflow state classification
- workflow state transitions
- seen-state behavior
- timeout behavior
- observability port emission behavior
- observability port query/filter behavior
- observability port failure behavior
- doctor health projection behavior

`atm` tests:
- clap parsing
- JSON output shape
- human-readable output snapshots
- send/read integration behavior
- `atm log` integration behavior
- `atm doctor` integration behavior
