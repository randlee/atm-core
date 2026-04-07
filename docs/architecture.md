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
- `teams`
- `members`

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
- ATM-owned config semantics for baseline roster, alias resolution, and
  runtime-identity precedence

`sc-observability` should own as much generic functionality as possible:
- emission
- record storage and retention policy
- historical query
- follow/tail
- severity filtering
- structured field filtering
- runtime health reporting

Phase K delivered the ATM-side integration work. Phase L now governs the
remaining release-hardening, boundary cleanup, and validation needed before
initial release.

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
- the retained release-critical team recovery surface is limited to:
  - `teams`
  - `members`
  - `teams add-member`
  - `teams backup`
  - `teams restore`
- broader historical team lifecycle/orchestration commands remain out of scope

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

pub struct LogFieldMap(BTreeMap<LogFieldKey, LogFieldValue>);

pub struct AtmJsonNumber(String);

pub enum LogFieldValue {
    Null,
    Bool(bool),
    String(String),
    Number(AtmJsonNumber),
    Array(Vec<LogFieldValue>),
    Object(LogFieldMap),
}
```

Architectural rules:
- `LogFieldKey` replaces raw field-name strings at the public observability
  boundary
- `AtmJsonNumber` replaces raw numeric `serde_json` values at the public
  observability boundary
- `LogFieldValue` and `LogFieldMap` replace raw `serde_json::Value` /
  `Map<String, Value>` in `LogFieldMatch` and `AtmLogRecord`
- these ATM-owned types must serialize to the same JSON shape the CLI exposes
  today; the boundary cleanup is a Rust API cleanup, not a CLI wire-format
  redesign
- conversion to and from raw `serde_json` values remains centralized inside
  `atm-core`

### 4.5 Observability Construction Contract

`CliObservability` (atm crate) should expose one structured construction path
for initial release, and `CliObservabilityOptions` is also owned by the `atm`
crate:

```rust
pub struct CliObservabilityOptions {
    pub stderr_logs: bool,
}

impl CliObservability {
    pub fn new(home_dir: &Path, options: CliObservabilityOptions) -> Result<Self, AtmError>;
}
```

Architectural rules:
- the top-level `init(stderr_logs)` helper may remain as a CLI convenience, but
  it should delegate to `CliObservability::new(...)`
- dynamic dispatch via `Box<dyn ObservabilityPort + Send + Sync>` remains
  acceptable for initial release
- the current sealed-trait pattern remains acceptable for initial release
- `DoctorCommand` injectability is explicitly deferred unless implementation
  surfaces a concrete need

### 4.6 Identity And Alias Projection

ATM must distinguish canonical routing identity from the Claude-facing sender
projection.

Architectural rules:
- runtime identity resolves from explicit CLI override, hook identity, or
  `ATM_IDENTITY`, not repo-local `[atm].identity`
- ATM-owned aliases are input shorthands that resolve to canonical member names
- same-team messages keep current canonical sender projection behavior
- cross-team messages may project an alias-friendly sender in the persisted
  `from` field for Claude-facing ergonomics
- whenever cross-team alias projection is used, ATM must also persist
  canonical sender identity in `metadata.atm.fromIdentity`
- self-send checks, target validation, routing, and audit logic must use the
  canonical sender identity rather than the display-oriented `from` projection
- ATM-owned post-send hooks are sender-scoped best-effort helpers, not part of
  the atomic send boundary
- the hook runs only after a successful non-`dry-run` send
- relative post-send-hook paths resolve from the discovered `.atm.toml`
  directory and execute with that same directory as the working directory
- the hook receives inherited environment plus one ATM-owned JSON payload in
  `ATM_POST_SEND`
- hook failure or timeout never rolls back a successful send

## 5. Persisted Schema

### 5.1 Team Config

The rewrite reuses the existing team config schema where feasible.

Only a small subset is required by the retained surface:
- member roster
- enough member metadata to preserve round-trips when present
- bridge remote host configuration needed for origin-file merge when present

ATM config and team-launch config are distinct concerns:
- ATM-owned config uses the `[atm]` section of `.atm.toml`
- launcher-owned sections such as `[rmux]` and future `[scmux]` remain outside
  the `atm-core` runtime config boundary and are ignored by ATM
- `[atm].team_members` is the ATM-owned baseline roster for doctor/orchestration
  checks
- `[atm].aliases` is the ATM-owned shorthand map for canonical agent names
- `[atm].post_send_hook` and `[atm].post_send_hook_members` are ATM-owned
  best-effort sender-scoped automation settings
- `[atm].identity` is obsolete in the retained multi-agent model and must not
  participate in runtime identity resolution

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
- cross-team alias projection stores canonical sender identity in
  `metadata.atm.fromIdentity`
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

#### 6.1.1 Idle-Notification Lifecycle

- message classification first attempts to parse the persisted `text` field as
  JSON and treat the message as an idle notification when the parsed object has
  `type == "idle_notification"`
- if parsing fails, or `type` differs, the message is classified as a normal
  message
- when a newly appended message is classified as an idle notification, the
  mailbox append boundary removes any older unread idle notification from the
  same sender in the same inbox before appending the new record
- `atm clear --idle-only` remains manual backlog cleanup, not the primary
  lifecycle path

Deferred follow-on work:
- read-time auto-purge of displayed idle notifications
- daemon-side idle-notification removal behavior

#### 6.1.2 Task-Assignment Classification

- classification uses the same text-field JSON detection pattern and treats a
  message as a task assignment when the parsed object has
  `type == "task_assignment"`
- because the Claude Code schema is fixed, classification must populate
  `extra["task_id"]` and `extra["priority"]` from the parsed text-field JSON
  rather than extending `MessageEnvelope` with new top-level fields
- final field naming and task-subsystem semantics remain coordinated with the
  future `arch-ctask` task subsystem design; see `atm-core` issue `#17`
- task-assignment extraction remains deferred until the `arch-ctask` subsystem
  is defined

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
  except for the explicitly documented cross-team alias projection carve-out on
  `from`, which also requires canonical sender identity in
  `metadata.atm.fromIdentity`

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

`atm-core` owns the ATM-specific event and query vocabulary needed for ATM’s
messaging workflows, retained-log query/follow, and doctor readiness.

`atm` owns the concrete `sc-observability` integration and CLI-facing routing
decisions such as `--stderr-logs`.

Future hook- or `schooks`-driven observability orchestration remains out of
scope for the initial ATM release and must not be inferred from this boundary.

### 6.6 Log Service

Public entrypoints:

- `ObservabilityPort::query(query: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError>`
- `ObservabilityPort::follow(query: AtmLogQuery) -> Result<LogTailSession, AtmError>`

ATM CLI surfaces such as `atm log snapshot`, `atm log filter`, and `atm log tail`
consume those boundary methods directly rather than routing through a separate
`log::query_logs(...)` or `log::tail_logs(...)` wrapper.
`AtmLogQuery` contains:
- mode
- level filters
- field matches
- time window
- limit

`AtmLogSnapshot` contains:
- returned records
- truncation flag when the shared query source truncates results

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
- current team member roster from `config.json`
- observability health

`DoctorFinding` contains:
- severity
- code
- message
- remediation

The report model should reuse the current doctor command’s severity/finding structure where useful, but local checks replace daemon checks.

Roster output rules:
- show all current `config.json` members in doctor output
- show baseline `[atm].team_members` first
- show `team-lead` first among the baseline members when present
- show extra runtime members after the baseline set

### 6.8 Team Recovery Services

The retained release-critical local team surface is intentionally narrow.

ATM-owned public entrypoints should cover:
- local team discovery
- local member listing
- local `add-member`
- local team backup
- local team restore

Architectural rules:
- these services are local file/config/inbox operations; they must not depend
  on daemon orchestration or runtime spawning
- `teams` list is discovery-oriented and should remain deterministic over the
  ATM home directory
- `add-member` is the retained local roster-repair path and must reject
  duplicates before mutating config
- `backup` snapshots current team config, inboxes, and the ATM team task
  bucket into a timestamped snapshot directory
- `restore` is a local recovery path and must:
  - preserve the current team-lead entry and `leadSessionId`
  - restore only missing non-lead members
  - clear runtime-only restored-member state before persistence
  - restore non-lead inboxes from the chosen snapshot
  - recompute `.highwatermark` from the maximum restored task id
  - support a dry-run path without making changes
- Claude Code project task-list restoration remains separate from the retained
  ATM team backup/restore surface

### 6.9 Members Service

The retained `members` surface is a local roster inspection service.

Architectural rules:
- it must succeed without daemon or hook-only state
- it must load the roster from local team config
- it should order members deterministically, with `team-lead` first when
  present
- it may surface persisted member metadata already present in config
- later hook/session enrichment may be layered on without changing the base
  local verification purpose of the command

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
3. inspect ATM config for obsolete fields such as `[atm].identity`
4. verify local team/mailbox/config paths
5. verify hook identity availability
6. compare baseline `[atm].team_members` against `config.json.members`
7. verify observability initialization and health
8. verify observability query readiness for `atm log`
9. assemble findings, recommendations, and ordered roster output
10. render report

## 12. Mailbox Storage

The mailbox layer owns:
- tolerant reads
- atomic append
- duplicate suppression
- conflict merge
- origin-inbox merge
- atomic workflow-state updates
- atomic clear-set replacement
- sender-scoped idle-notification dedup inside the atomic append boundary

The mailbox layer does not own selection policy, display buckets, output formatting, log query behavior, or doctor diagnostics.

## 13. Identity And File Policy

### 13.1 Hook Identity

Hook-file identity is retained because it is a current non-daemon convenience path for send/read identity resolution.

Only hook identity resolution is required for the rewrite. Session-resolution paths that exist only to bridge runtime/daemon ambiguity are not required.

Repo-local config identity is not retained as a runtime fallback. In the
multi-agent model, runtime identity must come from explicit CLI override,
hook identity, or `ATM_IDENTITY`. An obsolete `[atm].identity` field may be
diagnosed by doctor, but it must not control sender/actor resolution.

When `ATM_POST_SEND` is set for a configured post-send hook, the payload must
contain:
- `from`
- `to`
- `message_id`
- `requires_ack`
- optional `task_id`

The post-send hook runs only after a successful non-`dry-run` send, and hook
failure or timeout never rolls back a successful send.

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

The retained implementation uses an ATM-owned emit/query/follow/health boundary
that projects shared observability behavior into ATM-owned types:

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

Initial-release boundary rulings:
- this boundary is intentionally ATM-local; it does not attempt to model future
  hook-driven or `schooks`-orchestrated observability concerns
- the health contract remains intentionally closed at:
  - `Healthy`
  - `Degraded`
  - `Unavailable`
- public ATM observability projections must not expose raw
  `serde_json::Value` / `Map<String, Value>` directly

### 14.2 Shared Crate Usage Rules

Implementation rules:

- `atm-core` remains concrete-crate-neutral and consumes only the injected
  boundary
- `atm` initializes the shared logger exactly once per process
- the shared file sink is the authoritative retained log store for `atm log`
- the shared console sink remains opt-in so it does not contaminate normal
  command output
- the initial-release dependency is the published crates.io version
  `sc-observability = "1.0.0"`

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
- `atm teams` integration behavior
- `atm members` integration behavior
