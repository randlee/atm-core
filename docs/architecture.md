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
- `ack`
- `clear`
- `log`
- `doctor`

## 1.1 Documentation Structure

Documentation structure is governed by
[`documentation-guidelines.md`](./documentation-guidelines.md).

This file owns product architecture. Crate-local architectural detail is being
moved into:

- [`docs/atm/architecture.md`](./atm/architecture.md)
- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)

## 2. Crate Boundaries

The product is implemented by two crates:

- `atm-core`
- `atm`

Product-level boundary rules:

- `atm-core` owns daemon-free ATM business logic.
- `atm` owns CLI parsing, dispatch, rendering, and bootstrap.
- `atm-core` must not own clap or terminal-formatting concerns.
- `atm` must not own mailbox, workflow, log-query, or doctor business logic.

Crate-local boundary detail is owned by:

- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)
- [`docs/atm/architecture.md`](./atm/architecture.md)

Schema ownership references:

- Claude Code-native message schema:
  [`claude-code-message-schema.md`](./claude-code-message-schema.md)
- ATM additive/interpreted message schema:
  [`atm-message-schema.md`](./atm-message-schema.md)
- legacy ATM read-compatibility schema:
  [`legacy-atm-message-schema.md`](./legacy-atm-message-schema.md)
- `sc-observability` schema ownership pointer:
  [`sc-observability-schema.md`](./sc-observability-schema.md)
- ATM-owned error-code registry:
  [`atm-error-codes.md`](./atm-error-codes.md)
- schema enforcement models:
  `tools/schema_models/claude_code_message_schema.py` and
  `tools/schema_models/atm_message_schema.py` and
  `tools/schema_models/legacy_atm_message_schema.py`

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

The shared API gap is no longer the blocker. The current blocker is ATM-side
integration.

Initial retained-command integration scope:
- `sc-observability-types`
- `sc-observability`

Deferred from the initial retained-command integration scope:
- `sc-observe`
- `sc-observability-otlp`

The controlling ATM-side implementation design is:
- [`docs/atm-core/design/sc-observability-integration.md`](./atm-core/design/sc-observability-integration.md)

## 3. Module Layout

Detailed crate/module layout is owned by the crate-level docs:

- [`docs/atm-core/modules/`](./atm-core/modules/)
- [`docs/atm/commands/`](./atm/commands/)

Product-level constraints that remain relevant here:

- no plugin framework
- no daemon client
- no runtime spawning layer
- no separate `tail` command in the initial rewrite
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
- `TaskId`
- `HomeDir`
- `AbsolutePath`
- `LogFieldKey`
- `LogFieldValue`

These are required to reduce repeated validation and remove stringly typed command paths.

### 4.2 Workflow State And Display Types

Canonical axis enums:

```rust
pub enum ReadState {
    Unread,
    Read,
}

pub enum AckState {
    NoAckRequired,
    PendingAck,
    Acknowledged,
}

pub enum MessageClass {
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

Ack activation mode:

```rust
pub enum AckActivationMode {
    PromoteDisplayedUnread,
    ReadOnly,
}
```

Display mapping is fixed:
- `MessageClass::Unread` -> `DisplayBucket::Unread`
- `MessageClass::PendingAck` -> `DisplayBucket::PendingAck`
- `MessageClass::Acknowledged` -> `DisplayBucket::History`
- `MessageClass::Read` -> `DisplayBucket::History`

### 4.3 Typestate Transition Model

Per `rust-best-practices`, legal workflow transitions should be encoded in the type system inside the core pipeline.

Private marker states:

```rust
pub struct UnreadReadState;
pub struct ReadReadState;
pub struct NoAckState;
pub struct PendingAckState;
pub struct AcknowledgedAckState;

pub struct StoredMessage<R, A> {
    // persisted fields + read-state marker + ack-state marker
}

impl StoredMessage<UnreadReadState, NoAckState> {
    pub fn display_without_ack(self) -> StoredMessage<ReadReadState, NoAckState>;
    pub fn display_and_require_ack(self, at: IsoTimestamp) -> StoredMessage<ReadReadState, PendingAckState>;
}

impl StoredMessage<UnreadReadState, PendingAckState> {
    pub fn mark_read_pending_ack(self) -> StoredMessage<ReadReadState, PendingAckState>;
}

impl StoredMessage<ReadReadState, PendingAckState> {
    pub fn acknowledge(self, at: IsoTimestamp) -> StoredMessage<ReadReadState, AcknowledgedAckState>;
}
```

There is no inverse transition on either axis.

The public axis enums and `MessageClass` are for reporting and filtering. The typestate markers enforce legal transitions inside `atm-core`.

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
- member roster
- enough member metadata to preserve round-trips when present
- bridge remote host configuration needed for origin-file merge when present

Team config loading must follow a narrow-scope recovery policy:
- compatibility-only schema drift may use deterministic defaults at the schema
  boundary
- malformed member records should be isolated at member scope only when the
  remaining roster is still trustworthy
- missing `config.json` is a distinct `missing-document` condition, not a parse
  error
- root-document corruption or invalid root structure remains a command error
- identity and routing fields must never be guessed to keep commands running

Diagnostics for team config failures must preserve:
- failure class when known
- file path
- member or collection scope when known
- parser line and column when available
- original parser cause for operator repair

Sample operator-facing repair cases live in
[`persisted-data-repair.md`](./persisted-data-repair.md).

### 5.2 Inbox Message

Current persisted inbox superset may contain:
- Claude-native baseline fields:
  - `from`
  - `text`
  - `timestamp`
  - `read`
  - `summary`
  - optional producer field `color`
- legacy ATM top-level additive fields such as:
  - `source_team`
  - `message_id`
  - `pendingAckAt`
  - `acknowledgedAt`
  - `acknowledgesMessageId`
- shared/de facto interpreted fields such as:
  - `taskId`
- forward metadata container:
  - `metadata`
- unknown fields

Schema ownership split:

- Claude-native baseline fields are documented in
  [`claude-code-message-schema.md`](./claude-code-message-schema.md)
- legacy ATM top-level additive compatibility fields are documented in
  [`legacy-atm-message-schema.md`](./legacy-atm-message-schema.md)
- forward ATM machine-readable schema is documented in
  [`atm-message-schema.md`](./atm-message-schema.md)

Forward architectural rules:

- new ATM-only machine-readable data belongs in `metadata.atm`
- legacy top-level ATM fields remain read-compatible but are deprecated for new
  write behavior
- forward ATM-authored alert metadata, including legacy `atmAlertKind` and
  `missingConfigPath`, belongs under `metadata.atm` as
  `metadata.atm.alertKind` and `metadata.atm.missingConfigPath`
- ATM may enrich a Claude-native stored message by adding `metadata.atm`
  without rewriting the native Claude fields
- the current live design still uses a shared inbox surface; a separate
  ATM-native inbox is intentionally deferred to a later architecture phase

Current-phase constraint:

- the current runtime send/alert write path may continue writing legacy
  top-level alert fields during the compatibility period
- the metadata.atm alert placement defined above is the forward architectural
  target and must not be partially implemented without the corresponding
  migration sprint and tests
- the owning design rationale for this migration remains
  [`atm-core/design/dedup-metadata-schema.md`](./atm-core/design/dedup-metadata-schema.md)
  §2.2 and §3.3
Canonical read and ack axes are derived from persisted fields and not serialized separately.

Invariant:
- legacy top-level `message_id` values may be UUID or absent
- forward ATM metadata `messageId` values must be ULID
- write-path schema enforcement must reject placing ULID identifiers in the
  legacy top-level `message_id` slot and must reject placing UUID identifiers
  in forward `metadata.atm.messageId`
- read-path validation failure for those ATM-owned fields must log a warning,
  treat the malformed ATM-owned field as absent for ATM semantics, and continue
  processing the message when the Claude-native envelope remains usable
- when ATM authors a new ULID `messageId`, the persisted message `timestamp`
  must be derived from that ULID creation time so identifier ordering and
  timestamp ordering are aligned
- legacy or externally imported records may still lack ATM machine identifiers
- such records must be preserved as-is until enriched

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
- requires-ack flag
- optional task id
- dry-run flag

`SendMessageSource` variants:
- inline text
- stdin text
- file reference

`SendOutcome` fields:

| Field | Type | Description |
| --- | --- | --- |
| `action` | `&'static str` | Stable send action marker. |
| `team` | `String` | Resolved target team. |
| `agent` | `String` | Resolved target recipient. |
| `sender` | `String` | Resolved sender identity. |
| `outcome` | `&'static str` | Delivery result such as `sent` or `dry_run`. |
| `message_id` | `Uuid` | ATM-authored UUID v4 for the send operation. |
| `requires_ack` | `bool` | Whether the message requires acknowledgement. |
| `task_id` | `Option<String>` | Optional task identifier persisted on the message. |
| `summary` | `Option<String>` | Generated or caller-supplied summary text. |
| `message` | `Option<String>` | Rendered message body for dry-run output. |
| `warnings` | `Vec<String>` | Actionable degraded-mode warnings surfaced when send succeeds under a permitted fallback condition. |
| `dry_run` | `bool` | Whether the send was executed as a dry run. |

The file-reference path may be rewritten through the file policy layer.

The CLI JSON output mirrors the current contract.

Normal send JSON output includes:
- `action = "send"`
- `team`
- `agent`
- `outcome`
- `message_id`
- `requires_ack`
- `task_id`
- `warnings` when send completed in a degraded but permitted mode

Dry-run send JSON output includes:
- `action = "send"`
- `agent`
- `team`
- `message`
- `dry_run = true`
- `requires_ack`
- `task_id`
- `warnings` when dry-run surfaces degraded send conditions

Send ordering rules:
- resolve target address, team existence, and agent membership as one address-resolution stage before mailbox path selection
- enter the atomic append boundary before final inbox mutation
- validate message text inside the atomic append boundary
- current legacy top-level `message_id` generation remains supported for live
  compatibility
- forward metadata schema generation must create the ATM ULID `messageId`
  first and derive the persisted message `timestamp` from it
- perform duplicate suppression and final append inside the same atomic append boundary

Missing-team-config fallback is limited to `send`:
- fallback applies only when `config.json` is missing and the target inbox
  already exists
- malformed `config.json` remains a command error
- fallback must surface an actionable sender warning
- fallback may send a best-effort repair notice to `team-lead`
- repair notices must be deduplicated by unresolved condition so repeated sends
  do not flood inboxes

### 6.2 Read Service

Public entrypoint:

`read::read_mail(query: ReadQuery, observability: &dyn ObservabilityPort) -> Result<ReadOutcome, AtmError>`

`ReadQuery` contains:
- home directory
- current directory
- actor override
- optional target address
- team override
- selection_mode
- seen_state_filter
- seen_state_update
- ack_activation_mode
- limit
- sender_filter
- timestamp_filter
- optional timeout

`seen_state_filter` is false when `--no-since-last-seen` is set. `--all` bypasses this filter regardless of the stored value.

`seen_state_update` is false when `--no-update-seen` is set.

Timeout rule:
- if the requested selection is already non-empty after filtering and selection-mode application, return immediately
- otherwise wait for a newly eligible message until the timeout expires

`ReadOutcome` contains:
- action
- resolved team
- resolved agent
- selection_mode
- history_collapsed
- mutation_applied
- messages
- bucket_counts

`ReadOutcome.bucket_counts` exposes:
- unread
- pending_ack
- history

Read deduplication rule:
- collapse multiple entries with the same non-null `message_id` to the most
  recent entry before bucket selection and output rendering
- when timestamps tie, keep the later encountered inbox record

Read/enrichment rule:
- when a message needs ATM workflow semantics but lacks ATM-owned machine
  metadata, ATM may enrich the original stored message additively
- enrichment must be idempotent and must not rewrite native Claude fields

The read service derives `MessageClass` from `(ReadState, AckState)` and applies display-bucket selection to the derived class, not to raw persisted fields.

For merged inbox surfaces, any displayed-message mutation must be written back to the physical inbox file that contributed the displayed record. The merged view is a read projection, not a synthetic write target.

The CLI JSON output mirrors the current contract:
- `action`
- `team`
- `agent`
- `messages`
- `count`
- `bucket_counts`
- `history_collapsed`

### 6.3 Ack Service

Public entrypoint:

`ack::ack_mail(request: AckRequest, observability: &dyn ObservabilityPort) -> Result<AckOutcome, AtmError>`

`AckRequest` contains:
- home directory
- current directory
- actor override
- team override
- source message id
- reply body

`AckOutcome` contains:
- action
- resolved team
- resolved agent
- source message id
- optional task id from the acknowledged message
- reply target
- reply message id
- reply text

The ack service is responsible for the legal transition from `(Read, PendingAck)` to `(Read, Acknowledged)` plus the reply append.

When the source message came from an origin inbox file in the merged surface, the acknowledgement writeback must update that source file atomically rather than projecting the change onto a different inbox file.

### 6.4 Clear Service

Public entrypoint:

`clear::clear_mail(query: ClearQuery, observability: &dyn ObservabilityPort) -> Result<ClearOutcome, AtmError>`

`ClearQuery` contains:
- home directory
- current directory
- actor override
- optional target address
- team override
- optional age filter
- idle-only flag
- dry-run flag

`ClearOutcome` contains:
- action
- resolved team
- resolved agent
- removed total
- remaining total
- removal counters by class

Clear eligibility is computed from the two-axis model:
- clearable: `(Read, NoAckRequired)` and `(Read, Acknowledged)`
- non-clearable: every other combination

### 6.5 Observability Boundary

The observability boundary is a sealed `ObservabilityPort` (or equivalent injected interface) defined in `atm-core` and implemented in `atm`.

It is responsible for:
- command lifecycle emission
- log query
- log tail/follow
- observability health projection

The retained boundary must remain ATM-owned and must not leak shared
`sc-observability` types directly into `atm-core` public APIs.

`atm-core` owns the ATM-specific event and query vocabulary.

`atm` owns the concrete `sc-observability` integration.

### 6.6 Log Service

Public entrypoints:

- `ObservabilityPort::query(query: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError>`
- `ObservabilityPort::follow(query: AtmLogQuery) -> Result<LogTailSession, AtmError>`

ATM CLI surfaces such as `atm log snapshot`, `atm log filter`, and `atm log tail`
consume those boundary methods directly rather than routing through a separate
`log::query_logs(...)` or `log::tail_logs(...)` wrapper.

`AtmLogQuery` contains:
- mode
- level filter
- field matches
- time window
- limit

`AtmLogSnapshot` contains:
- resolved query
- snapshot ordering metadata
- returned records

`LogTailSession` is an owning stateful object that yields matching records from the shared observability follow API without exposing a public callback trait.

Ordering rules:
- snapshot queries return newest-first records before CLI output limits are rendered
- tail sessions yield records in follow arrival order

ATM must not parse daemon log files directly in this service.

### 6.7 Doctor Service

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
4. collapse duplicate `message_id` entries to the newest visible record
5. classify read axis, ack axis, and derived message class
6. apply sender and timestamp filters
7. apply seen-state filter unless selection is `All`
8. map derived message class to display bucket and apply selection mode
9. wait if `timeout` is set and the current selection is empty
10. sort newest-first and apply limit
11. apply legal read-axis and ack-axis transitions for displayed messages
12. persist state changes atomically
13. update seen-state when enabled
14. return outcome

This ordering is part of the architecture contract.

## 8. Ack Pipeline

The ack pipeline stages are:
1. resolve actor identity and own inbox
2. load the merged inbox surface and locate the source message
3. classify the source message into read and ack axes
4. require pending acknowledgement before mutation
5. resolve the reply target inbox from the source envelope
6. atomically apply the ack transition and append the reply
7. emit command lifecycle records
8. return outcome

## 9. Clear Pipeline

The clear pipeline stages are:
1. resolve actor identity and target inbox
2. load the persisted inbox surface
3. classify each message into read axis and ack axis
4. compute clear eligibility from the two-axis model plus pending-ack override
5. apply optional age and idle-only filters
6. atomically persist the kept set when not in dry-run mode
7. emit command lifecycle records
8. return outcome

## 10. Log Pipeline

The log pipeline stages are:
1. resolve the injected observability port implementation
2. map CLI filters into shared query/follow filters
3. query or follow records through the observability port
4. project ATM-owned record fields for CLI rendering
5. return records to the CLI layer

Shared `sc-observability` should own record storage, filtering, and follow mechanics. ATM should own only ATM-specific query defaults and field projections.

## 11. Doctor Pipeline

The doctor pipeline stages are:
1. resolve config and environment overrides
2. resolve effective team and identity inputs
3. verify local team/mailbox/config paths
4. verify hook identity availability
5. verify observability initialization and health
6. verify observability query readiness for `atm log`
7. assemble findings and recommendations
8. render report

## 12. Mailbox Storage

The mailbox layer owns:
- tolerant reads
- atomic append
- duplicate suppression
- conflict merge
- origin-inbox merge
- atomic workflow-state updates
- atomic clear-set replacement

The mailbox layer does not own selection policy, display buckets, output formatting, log query behavior, or doctor diagnostics.

## 13. Identity And File Policy

### 13.1 Hook Identity

Hook-file identity is retained because it is a current non-daemon convenience path for send/read identity resolution.

Only hook identity resolution is required for the rewrite. Session-resolution paths that exist only to bridge runtime/daemon ambiguity are not required.

### 13.2 File Policy

The current `send --file` behavior is retained:
- inspect Claude settings permissions when available
- if the referenced file is allowed, send a direct file reference
- otherwise copy to ATM share storage and rewrite the message body accordingly

## 14. Observability

`atm-core::observability` defines ATM event/query models plus the sealed `ObservabilityPort` boundary.

`atm` provides the concrete `sc-observability` implementation and injects it into core services.

Initialization:
- `atm` initializes logging once at process startup
- `atm` constructs the concrete observability port after startup initialization
- logging failures degrade to best-effort behavior for explicit mail commands

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
- task id
- outcome
- error class when applicable
- stable error code when applicable
- message count when applicable
- transition count when applicable

For explicit observability consumer commands:
- `atm log` depends on shared query/follow APIs
- `atm doctor` depends on shared health APIs
- failures in those consumer paths are command errors, not silently dropped events

### 14.1 Concrete Integration Shape

The current tracing-only emit adapter is temporary. The retained implementation
must expand to:

- ATM-owned `AtmLogQuery`
- ATM-owned `AtmLogRecord`
- ATM-owned `AtmLogSnapshot`
- ATM-owned `AtmObservabilityHealth`
- an ATM-owned synchronous `LogTailSession`

Required boundary responsibilities:

- `ObservabilityPort::emit(...)`
- `ObservabilityPort::query(...)`
- `ObservabilityPort::follow(...)`
- `ObservabilityPort::health(...)`

The exact ATM-owned projected types and object-safe follow-session split are
defined in:
- [`docs/atm-core/design/sc-observability-integration.md`](./atm-core/design/sc-observability-integration.md)

### 14.2 Shared Crate Usage Rules

Implementation rules:

- `atm-core` remains concrete-crate-neutral and consumes only the injected
  boundary
- `atm` initializes the shared logger exactly once per process
- the shared file sink is the authoritative retained log store for `atm log`
- the shared console sink remains opt-in so it does not contaminate normal
  command output
- until `sc-observability` is published, developer and CI builds may use a
  repo-local Cargo patch/path strategy against a sibling checkout; committed
  ATM docs and scripts must not require a user-specific absolute path

### 14.3 Failure Diagnostic Rules

Required diagnostic behavior:

- CLI bootstrap failures must be logged before process exit
- CLI parse/validation failures that occur before a core service runs must be
  logged before process exit
- retained command-service failures must emit structured failure diagnostics
  with stable ATM-owned error codes
- degraded recovery warnings that continue the command must also log stable
  error codes
- command success-only logging is insufficient for the retained architecture

## 15. Error Model

Root public error:

```rust
pub struct AtmError {
    pub code: AtmErrorCode,
    pub kind: AtmErrorKind,
    pub message: String,
    pub recovery: Option<String>,
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}
```

```rust
pub enum AtmErrorCode {
    // single central registry re-exported from crates/atm-core/src/error_codes.rs
}
```

Required families:
- config
- missing document
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
- a stable ATM-owned error code
- a stable class
- human-readable cause
- recovery guidance when the user can act

The single source of truth for ATM-owned error codes is:
- [`atm-error-codes.md`](./atm-error-codes.md)

Persisted-data errors should additionally carry file/entity/parser context so
CLI surfaces can report the exact failing document and scope.

## 16. Trait Policy

The initial rewrite should avoid public extension traits.

If a trait becomes necessary:
- prefer a sealed trait
- verify object safety before stabilization

## 17. Testing Strategy

`atm-core` tests:
- address parsing
- config precedence
- tolerant team-config parsing for compatibility-only schema drift
- precise persisted-data diagnostics for non-recoverable config failures
- bridge hostname resolution for merged inbox reads
- settings resolution
- hook identity resolution
- file policy behavior
- team membership validation
- tolerant inbox parsing
- origin-inbox merge
- atomic append behavior
- duplicate suppression
- read-time duplicate collapse by `message_id`
- workflow axis classification
- workflow axis transitions
- task-linked ack-required classification
- seen-state behavior
- timeout behavior
- ack transition behavior
- clear eligibility behavior
- pending-ack clear override behavior
- observability port emission behavior
- observability port query/filter behavior
- observability port failure behavior
- doctor health projection behavior

`atm` tests:
- clap parsing
- JSON output shape
- human-readable output snapshots
- send/read/ack/clear integration behavior
- `atm log` integration behavior
- `atm doctor` integration behavior
