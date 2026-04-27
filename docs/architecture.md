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

### 2.3 Release Publication Boundary

The `1.0` retained-surface release is a source-repo replacement of the old
`agent-team-mail` CLI/core publication path, not a new public package family.

Architectural rules:
- this repo becomes the source of truth for publishing:
  - `agent-team-mail`
  - `agent-team-mail-core`
- this repo does not publish its retained CLI/core release under the crate
  names `atm` or `atm-core`
- crate identity continuity for downstream users is preserved by package-name
  replacement while keeping the CLI binary name `atm`
- historical parity channels remain:
  - crates.io
  - GitHub Releases
  - Homebrew
- `winget` is not part of historical parity, but it is required in the new
  release architecture because Windows installation must be first-class for
  `1.0` without Rust tooling or manual archive extraction

Release-process ownership rules:
- release automation is repo-owned infrastructure, not ad hoc operator
  procedure
- the new repo must own:
  - release artifact manifest
  - preflight workflow
  - release workflow
  - release-gate script/helpers
  - release inventory generation and verification
  - Homebrew formula update automation
  - `winget` manifest/update automation and verification
- the `publisher` agent instructions are part of the release-control surface
  and must be ported into this repo with source-of-truth paths updated to the
  new repo layout and retained crate list

Release infrastructure notes:
- Homebrew continues to use the shared `randlee/homebrew-tap` repository and
  existing `Formula/agent-team-mail.rb` / `Formula/atm.rb` formulas
- `HOMEBREW_TAP_TOKEN` is a required secret for the `atm-core` repo before the
  ported Homebrew update automation can run successfully
- `winget` uses the same `randlee` publisher namespace proven in
  `claude-history`; the retained CLI package ID for this repo is
  `randlee.agent-team-mail`
- the ported `winget` flow uses the default GitHub workflow token and does not
  introduce a separate `winget`-specific secret requirement
- the release workflow should use
  `vedantmgoyal2009/winget-releaser@v2` against the Windows ZIP release asset
  and its SHA256 rather than inventing repo-specific manifest plumbing first
- the initial `winget` manifest submission is a one-time manual bootstrap
  action; recurring releases are workflow-driven after the package exists in
  `microsoft/winget-pkgs`
- release verification must treat `winget` submission success and manifest
  generation as the immediate release signal because Microsoft review normally
  delays public installability by 1-2 days

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

### 2.4 Shared Observability Boundary

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
- ATM-owned post-send hooks are best-effort recipient-scoped helpers, not part
  of the atomic send boundary
- the hook runs only after a successful non-`dry-run` send
- each `[[atm.post_send_hooks]]` rule binds one recipient selector and one
  command argv
- `recipient = "*"` acts as a wildcard match for all recipients
- multiple matching rules all execute, in config order
- relative post-send-hook paths resolve from the discovered `.atm.toml`
  directory and execute with that same directory as the working directory
- bare executable names use normal `PATH` lookup
- the hook receives inherited environment plus one ATM-owned JSON payload in
  `ATM_POST_SEND`
- the payload includes `from`, `to`, `sender`, `recipient`, `team`,
  `message_id`, `requires_ack`, and optional `task_id`
- the hook may optionally emit one structured result object on stdout with a
  declared log level, message, and optional structured fields; ATM parses it
  on a best-effort basis for post-send diagnostics
- absent or invalid hook-result stdout is ignored rather than treated as hook
  failure
- recipient non-match is silent
- retired flat hook keys and `[atm].post_send_hook_members` are configuration
  errors, not compatibility aliases
- hook-decision logging must preserve sender, recipient, matched rule selector,
  and final execution outcome for troubleshooting
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
- `[[atm.post_send_hooks]]` is the ATM-owned best-effort post-send automation
  surface
- retired flat hook keys and `[atm].post_send_hook_members` must fail fast
  with migration guidance
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

### 5.1.1 Deprecated `[atm].identity`

`[atm].identity` remains parse-compatible only as an obsolete migration field.
It is no longer part of runtime sender or actor resolution.

Current runtime contract:
- runtime identity resolves from explicit CLI override when supported, then
  hook identity, then `ATM_IDENTITY`
- if no runtime identity source is available, the command fails with
  `ATM_IDENTITY_UNAVAILABLE`
- `[atm].identity` is ignored for runtime resolution even when still present in
  `.atm.toml`

Deprecation and migration contract:
- `atm doctor` reports stale `[atm].identity` with
  `ATM_WARNING_IDENTITY_DRIFT`
- operator migration path is: remove `[atm].identity` and set `ATM_IDENTITY`
  in the active agent environment instead
- keeping the obsolete key temporarily is tolerated for migration diagnostics
  only; it must not change runtime behavior

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

Current compatibility rule:

- the current runtime send/alert write path may continue writing legacy
  top-level alert fields during the compatibility period
- the metadata.atm alert placement defined above is the forward architectural
  target and must not be partially implemented without the corresponding
  migration sprint and tests
- the owning design rationale for this migration remains
  [`atm-core/design/dedup-metadata-schema.md`](./atm-core/design/dedup-metadata-schema.md)
  §2.2 and §3.3

File-ownership rule:

- Claude-owned inbox content is not an ATM-owned source of truth for ATM-local
  workflow durability
- ATM may still have legacy compatibility write paths on the shared inbox
  surface, but those paths must be documented as compatibility behavior rather
  than a general pattern to copy
- ATM-owned machine state should converge on ATM-owned sidecars or equivalent
  ATM-owned persisted state when stronger write guarantees are required
- mailbox-local ATM workflow state now lives in the ATM-owned sidecar family at
  `.claude/teams/<team>/.atm-state/workflow/<agent>.json`
- `read`, `ack`, and `clear` project mailbox display state by joining
  Claude-owned inbox records with the ATM-owned workflow sidecar
- messages without a stable ATM identity remain compatibility-only and may
  still use the legacy inbox-local workflow fields until a later enrichment
  phase lands

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
- snapshot `~/.claude/teams/*/inboxes/*.lock` at doctor start and end; any lock
  path present in both snapshots is stale and should surface as
  `ATM_WARNING_STALE_MAILBOX_LOCK` with `rm -f <path>` recovery guidance

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
- inbox backup excludes transient mailbox `*.lock` sentinels, dotfiles, and
  restore markers
- `restore` is a local recovery path and must:
  - preserve the current team-lead entry and `leadSessionId`
  - restore only missing non-lead members
  - clear runtime-only restored-member state before persistence
  - restore non-lead inboxes from the chosen snapshot
  - sweep stale mailbox `*.lock` sentinels before restored inbox files are copied in
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

### 13.1 Hook Matching

When `ATM_POST_SEND` is set for a configured post-send hook, the payload must
contain:
- `sender`
- `recipient`
- `team`
- `from`
- `to`
- `message_id`
- `requires_ack`
- optional `task_id` when present

The post-send hook runs only after a successful non-`dry-run` send, executes
once when sender or recipient matching succeeds, may optionally emit one
structured stdout result for observability, and never rolls back a successful
send on failure or timeout.

Supported structured hook-result levels remain:
- `debug`
- `info`
- `warn`
- `error`

### 13.2 Identity Resolution

Hook-file identity is retained because it is a current non-daemon convenience
path for send/read identity resolution.

Only hook identity resolution is required for the rewrite. Session-resolution
paths that exist only to bridge runtime/daemon ambiguity are not required.

Repo-local config identity is not retained as a runtime fallback. In the
multi-agent model, runtime identity must come from explicit CLI override,
hook identity, or `ATM_IDENTITY`. An obsolete `[atm].identity` field may be
diagnosed by doctor, but it must not control sender/actor resolution.

### 13.3 File Policy

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


## 18. Mailbox File Locking (Phase M)

### 18.1 Problem Statement

`append_message` in `mailbox/mod.rs:23-27` performs an unlocked read-modify-write:

1. `read_messages(path)` — reads and deserializes the full inbox
2. `messages.push(envelope)` — appends the new record in memory
3. `atomic::write_messages(path, &messages)` — writes to temp file, fsyncs, renames over original

Step 3 is atomic with respect to partial writes but not concurrent callers. Two concurrent
callers can both complete step 1 before either reaches step 3; the later rename silently
overwrites the earlier, losing its appended message. The same race affects read writeback,
ack transition, and clear set replacement.

### 18.2 Locking Primitive Decision

**Decision: Use the `fs2` crate.**

Rationale:
- `fs2` provides `FileExt::lock_exclusive()` and `FileExt::try_lock_exclusive()` which map
  to `flock(2)` on Unix and `LockFileEx` on Windows
- 98M+ downloads, maintained, compatible with the project's MSRV
- avoids maintaining separate `cfg(unix)` / `cfg(windows)` implementations
- the current `atm-core` Cargo.toml already carries `libc` and `windows-sys`, but
  only as low-level building blocks, not as a cross-platform mailbox-locking API

Alternative rejected: direct `libc::flock` + `windows-sys::LockFileEx` — more control but
duplicates what `fs2` already provides correctly.

### 18.3 Lock Architecture

```
                      +-----------------------+
                      |   MailboxLockGuard     |
                      |  (RAII, Drop releases) |
                      +----------+------------+
                                 |
                      +----------v------------+
                      |   lock.rs::acquire()   |
                      |  open/create sentinel  |
                      |  fs2::try_lock_excl()  |
                      +----------+------------+
                                 |
             +-------------------+-------------------+
             |                                       |
    Unix: flock(fd, LOCK_EX)           Windows: LockFileEx(handle)
```

- **Sentinel**: `{inbox_path}.lock` — pid-bearing runtime artifact, created lazily,
  removed on `MailboxLockGuard` drop, and best-effort evicted when the recorded pid
  is no longer alive
- **Granularity**: per-inbox-file — concurrent sends to different recipients never contend
- **Lock lifetime**: acquired before `read_messages`, held through `atomic::write_messages`
  durability boundary (temp-file write, rename, and any parent-directory sync),
  then the sentinel is unlinked and the guard is released
- **Timeout**: bounded retry loop with `try_lock_exclusive()` + 50ms sleep, default 5s;
  on expiry returns `AtmError { code: MailboxLockTimeout }`
- **Error classification**: only genuine "lock busy" results participate in the
  retry loop. Non-contention I/O and OS failures from the lock path fail fast as
  `MailboxLockFailed` with filesystem/permissions recovery guidance instead of
  being collapsed into a timeout.
- **Cooperative limitation**: `fs2` locks are advisory and only coordinate ATM
  processes that participate in the same locking protocol. Direct file edits or
  other tools that bypass ATM locking are outside the protection boundary. This
  is an accepted limitation for the ATM shared-inbox model.

### 18.3.1 Stale-Sentinel Sweep Predicate

The current `path.extension() == "lock"` filter is too narrow because it misses
rotated sentinels such as `inbox.json.lock.old`. The executed P.10 design must
match only filenames that still carry the sentinel suffix chain:

```rust
let is_lock_sentinel_candidate = path
    .file_name()
    .and_then(|name| name.to_str())
    .is_some_and(|name| name.ends_with(".lock") || name.contains(".lock."));
```

Why this exact predicate:
- `ends_with(".lock")` preserves the ordinary live sentinel path
- `contains(".lock.")` catches rotated forms such as `.lock.old` and
  `.lock.replaced`
- basename-only matching avoids broad false positives from parent directories
- rejecting generic `contains("lock")` avoids matching unrelated files such as
  `locksmith.txt`

Eviction remains conservative:
- read the candidate contents as the documented `pid[:token]` owner record
- if parsing fails, leave the file in place
- if `process_is_alive(pid)` is true, leave the file in place
- only then attempt removal

This is still best-effort cleanup, not a second ownership protocol. The actual
authority boundary remains the later `fs2` advisory lock plus the existing
`lock_path_matches_file(...)` identity recheck after acquisition.

Platform note:
- Windows may not permit renaming a live locked sentinel the same way Unix
  does, so the broadened sweep is not a live-handoff mechanism
- the predicate exists to clean up crash leftovers, repair leftovers, or
  externally rotated sentinel artifacts that otherwise evade the old exact
  `.lock` extension test

### 18.3.2 Read-Only Filesystem Classification

P.10 should add a dedicated read-only-filesystem mailbox-lock code instead of
overloading the generic non-contention lock failure bucket.

Required platform mapping:
- Linux: `libc::EROFS` (`30`)
- macOS: `libc::EROFS` (`30`)
- Windows: `windows_sys::Win32::Foundation::ERROR_WRITE_PROTECT` (`19`)

The classification helper belongs at the lock-path error-conversion boundary,
not duplicated ad hoc at individual call sites. The intended shape is:

```rust
fn is_readonly_filesystem_error(error: &io::Error) -> bool
```

and then a shared mapper such as:

```rust
fn mailbox_lock_path_error(
    operation: &'static str,
    lock_path: &Path,
    error: io::Error,
) -> AtmError
```

Call-graph decisions:
- `open_lock_file(...)` maps read-only failures directly to
  `MailboxLockReadOnlyFilesystem`
- `write_lock_owner_record(...)` maps both truncate and write failures through
  the same helper
- `remove_lock_sentinel_with_retry(...)` explicitly does not retry read-only
  failures before the current permission-denied/backoff logic
- public `sweep_stale_lock_sentinels(...)` surfaces the read-only diagnostic to
  the caller rather than logging and continuing
- pre-acquisition stale eviction inside `acquire(...)` propagates the
  read-only diagnostic when the cleanup path hits it, because subsequent owner
  record writes cannot succeed on the same mount
- `MailboxLockGuard::drop` still warns only, because the successful mailbox
  mutation has already completed and `Drop` cannot change the command result

Recommended recovery text:
- message includes the attempted operation and lock path
- recovery tells the operator to remount or move the ATM home to a writable
  filesystem before retrying, not merely to wait for another process

Reason for a new code instead of enriching `MailboxLockFailed`:
- read-only filesystem state is a stable, operator-actionable class with
  different remediation from ACL failures or transient path I/O
- the retry policy must branch on this distinction
- QA and integration tests need a stable machine-readable contract for it

### 18.4 Integration: Single-File Helper + Multi-File Lock Set

`append_message` is a true single-file read-modify-write and should use one shared helper:

```rust
pub fn locked_read_modify_write<F>(
    path: &Path,
    timeout: Duration,
    mutate: F,
) -> Result<(), AtmError>
where
    F: FnOnce(&mut Vec<MessageEnvelope>) -> Result<(), AtmError>,
{
    let _guard = lock::acquire(path, timeout)?;
    let mut messages = read_messages(path)?;
    mutate(&mut messages)?;
    atomic::write_messages(path, &messages)
}
```

That helper is the right shape for:
- `append_message`
- the missing-config team-lead notice path, because it also calls `append_message`

It is **not** sufficient by itself for `read`, `ack`, and `clear`, because those
commands call `load_source_files(...)` and compute a merged surface across the
requested inbox plus any origin inboxes before writing back. To make those paths
concurrency-safe, Phase M needs a second abstraction:

```rust
pub fn acquire_many_sorted(
    paths: impl IntoIterator<Item = PathBuf>,
    timeout: Duration,
) -> Result<Vec<MailboxLockGuard>, AtmError>
```

Required usage:
- discover the full source-file set first
- dedupe paths and sort them deterministically by canonical path string
- source-file discovery must finish before the first inbox read
- legitimately absent inbox paths at discovery time are excluded from the lock
  set rather than locked speculatively
- source discovery must fail closed for mutation commands: unreadable
  `read_dir(...)` entries or equivalent enumeration faults are treated as source
  set instability, not as warnings that can be skipped
- source discovery faults abort the command before lock acquisition; mutation
  commands never attempt a partial lock set after a discovery failure
- acquire all locks against one total timeout budget
- if any acquisition fails, drop every earlier lock immediately and abort before
  any source-file read
- if a discovered file disappears or becomes unreadable after lock planning but
  before `load_source_files(...)` completes, abort without persisting any
  partial state; this remains a normal operator-actionable file-read failure,
  not a partial-lock degraded mode
- then call `load_source_files(...)`
- hold every guard until every source writeback completes

This intentionally preserves a single logical merged-surface decision boundary
for `read`, `ack`, and `clear`. Those commands are not allowed to degrade into
partial-lock best-effort mutation, because doing so would mix snapshots from
different logical times and make writeback correctness nondeterministic.

### 18.4.1 Cooperative Locking Contract For `ack_mail`

`ack_mail` sometimes needs to mutate a source inbox set and append the reply to
another inbox that was not part of the initial actor-source set. The accepted
implementation does not use a subset-lock then upgrade-to-superset sequence.
Instead it uses:

1. an unlocked observational snapshot of the actor-source set
2. unlocked validation of the pending-ack state and reply inbox path
3. one final acquisition of the full sorted superset that includes the reply
   inbox
4. re-discovery of source paths, reload of current source files, and
   re-validation of the pending-ack state under that final lock set
5. persistence of both the updated source message and reply while the superset
   locks are still held

This avoids the deadlock risk of trying to expand a held subset into a larger
sorted lock set. The unlocked preflight is acceptable only because `ack_mail`
does not mutate from that preflight snapshot: the shared commit helper reloads
and re-validates both the source-path set and the pending-ack state under the
final superset lock before writing anything. If the state drifted, `ack_mail`
aborts instead of mutating a stale snapshot.

| Caller | Lock required |
|--------|--------------|
| `append_message` | `locked_read_modify_write` |
| `send` missing-config notice append | `append_message` coverage |
| source discovery fault (`read` / `ack` / `clear`) | abort before lock acquisition; no partial lock set attempted |
| `read` writeback | initial selection load is unlocked; acquire the multi-file lock set only for the reload + writeback phase |
| `ack` transition + reply | unlocked preflight, then one final cooperative superset lock including reply inbox; see §18.4.1 |
| `clear` set replacement | multi-file lock set held from first read through persist |
| `read_messages` (read-only, no writeback) | No |

### 18.4.2 Read-Only Vs Read-Modify-Write

ATM now treats mailbox access as two distinct patterns:

1. Read-only snapshot:
   - discover source inbox paths
   - load and classify the current merged surface without mailbox locks
   - use this for display-only selection and timeout polling

2. Read-modify-write:
   - re-acquire the deterministic source lock set only when a command is about to
     persist mailbox state
   - re-discover and re-validate the source path set under lock
   - reload the mailbox state, recompute selection, apply transitions, and
     persist while the lock set is still held

This keeps non-mutating reads out of the lock path while preserving a stable
writeback boundary for commands that actually rewrite inbox files.

Executed command mapping:
- `read` uses an unlocked observational snapshot for display selection and
  timeout polling, then enters the shared lock+reload+recompute path only when
  display-state mutation is actually required
- `ack` uses an unlocked preflight to resolve the reply target and candidate
  source message, then acquires one final sorted superset lock and re-validates
  the pending-ack state under that lock set before writing source/reply state
- mutating `clear` acquires the shared lock plan before its mutating reread and
  holds it through removal computation, mailbox replacement, and workflow-state
  updates; `clear --dry-run` remains observational only

### 18.4.3 Executed Mailbox Workflow Migration

Phase P completed the mailbox workflow-state migration. P.4 delivered the
sidecar move, and the current architecture documents the post-P.5 executed
state.

Current executed rule:
- ATM-owned workflow durability for identified mailbox messages is written to
  `.claude/teams/<team>/.atm-state/workflow/<agent>.json`
- `send` authors forward `metadata.atm.messageId` ULIDs for ATM-authored
  records and seeds the corresponding sidecar entry
  - QA criterion (a) for ULID assignment is verified through `send_mail`
    coverage; the helper that writes `metadata.atm.messageId` remains an
    internal `pub(crate)` workflow API and is not exposed to integration tests
- `read` projects mailbox display state from the sidecar and only rewrites the
  inbox file for legacy compatibility records that still lack a stable ATM
  identity
- `ack` writes the reply inbox file plus the source/reply workflow-state files
  under one deterministic lock plan
- `clear` classifies removable messages from the projected workflow view and
  removes matching workflow-state entries when the inbox record is deleted

Current executed limitation:
- `send` and the missing-config team-lead notice path still seed workflow state
  via an atomic owner-routed `load -> mutate -> save` sequence instead of a
  dedicated freshness-proving helper
- that means the sidecar family is already the source of truth, but concurrent
  same-recipient send-side seeding is not yet hardened to the same
  lock/reload/recompute standard used by mailbox read/ack/clear
- P.6 is the tracked hardening continuation for that specific gap

### 18.5 New Error Codes

- `MailboxLockFailed` / `ATM_MAILBOX_LOCK_FAILED` — lock-path creation,
  open, or acquisition failed for a non-contention filesystem or OS reason
- `MailboxLockReadOnlyFilesystem`
  / `ATM_MAILBOX_LOCK_READ_ONLY_FILESYSTEM` — the lock path or lock sentinel
  lives on a read-only filesystem, so ATM cannot create, update, or remove the
  required mailbox-lock artifact
- `MailboxLockTimeout` / `ATM_MAILBOX_LOCK_TIMEOUT` — lock not acquired within timeout
- New `AtmErrorKind::MailboxLock` variant in `error.rs`

### 18.6 Shared Mutable File Atomicity

Mailbox locking closes the concurrent lost-update race for inbox files, but it
is only one part of the persistence contract. Phase M also treats atomic file
replacement as a repo-wide rule for shared mutable ATM-owned structured state.

Scope:
- live inbox files
- team `config.json`
- ATM-owned task-bucket files restored or rewritten by team recovery
- `.highwatermark`
- shared persisted coordination/state files such as send-alert or
  restore-progress markers when they carry ATM-owned operator state
- any future ATM-owned JSON/JSONL/state file rewritten by more than one ATM
  process or operator workflow

Architectural rule:
- no live shared mutable structured file may be rewritten in place
- writers must use a temp-file + fsync + rename style replacement on the same
  filesystem, or a documented equivalent with the same atomicity guarantee
- for rename-based replacement, the helper must also fsync the parent directory
  after the rename whenever the platform supports directory-sync semantics; this
  is the Phase M crash-durability boundary for mailbox/config/shared-state
  replacement
- `atm-core` must own one shared low-level atomic persistence primitive and a
  small set of typed writer helpers layered on top of it, rather than open-code
  file replacement logic at individual call sites
- existing helpers such as `atomic::write_messages(...)` and
  `write_team_config(...)` are the preferred integration points; new shared
  state added by Phase M should extend that helper pattern with typed helpers
  for task-bucket, highwatermark, and shared coordination files instead of
  open-coding direct `fs::write(...)` mutations

Single-write-path guardrail:
- each live file family should have one owning write boundary
- low-level atomic replacement belongs in `persistence.rs`
- file-family semantics belong in one owner-layer helper such as mailbox or
  team-admin
- command handlers should express intent and call the owner-layer helper rather
  than assemble write mechanics locally
- if a new write precondition appears, the default response should be to extend
  the shared helper or owner-layer helper rather than introducing a parallel
  write path

Current owner-layer boundaries:
- Claude-owned inbox compatibility surface:
  `mailbox::store::observe_source_files(...)` for observational snapshots,
  `mailbox::store::with_locked_source_files(...)` for shared mailbox
  read/ack/clear lock+reload orchestration, and
  `mailbox::store::commit_mailbox_state(...)` /
  `mailbox::store::commit_source_files(...)` as the persistence leaf
- ATM-owned source-of-truth state:
  `workflow::{load_workflow_state(...), save_workflow_state(...),
  project_envelope(...), remember_initial_state(...),
  apply_projected_state(...), remove_message_state(...)}`,
  `read::seen_state::save_seen_watermark(...)`,
  `send::alert_state::{register_missing_team_config_alert(...),
  clear_missing_team_config_alert(...), save(...)}`, and
  `team_admin::write_team_config(...)`
- ATM-owned restore/task state:
  `team_admin::restore::restore_task_state_from_backup(...)`,
  `team_admin::restore::write_restore_marker(...)`, and
  `team_admin::restore::clear_restore_marker(...)`
- staging/scratch artifacts:
  `team_admin::restore::prepare_restore_workspace(...)` and
  `team_admin::restore::cleanup_restore_workspace(...)`

Current architectural limitation:
- mailbox replacement is atomic and lock-coordinated for concurrent ATM
  writers, but it is not yet compare-and-swap against non-cooperating Claude
  writers
- therefore the current shared-inbox rewrite path is still a compatibility
  boundary, not the ideal long-term source-of-truth architecture for ATM-local
  workflow state
- separately, send-side workflow seeding still lacks a dedicated freshness
  boundary across concurrent same-recipient sends; that is a post-P.5 hardening
  gap rather than a reason to move workflow durability back into Claude-owned
  inbox records

This rule intentionally applies beyond mailbox files so future work does not
reintroduce partial-write or torn-state risks through backup/restore or shared
auxiliary state paths.

### 18.6.1 Deterministic Locking-Test Strategy

The follow-up locking fixes require failure-path tests, but those tests must not
depend on races or hang-prone construction.

Test strategy:
- contention tests use a helper thread/process that acquires the target lock and
  signals readiness through a channel or barrier
- the command under test uses a short bounded lock timeout
- assertions use `recv_timeout(...)`, elapsed-time ceilings, and scoped guard
  teardown instead of indefinite `join()`/sleep loops
- source-discovery fault tests use a deterministic seam (for example, an
  injected directory-entry iterator/fault source) to force an unreadable origin
  entry without depending on filesystem timing or permission quirks
- non-contention lock error tests use a deterministic seam around the lock
  attempt/classifier rather than trying to synthesize platform-specific OS
  failures opportunistically
- durability tests validate helper sequencing and error propagation through
  deterministic seams; they do not attempt literal crash simulation in unit or
  integration test runs

This is intentionally stricter than the Phase M success-path deadlock tests so
CI remains bounded and repeatable across macOS, Linux, and Windows.

## 19. Restore Transaction Atomicity (Phase M)

### 19.1 Problem Statement

`restore_team` in `team_admin.rs` currently mutates in this order:
1. Copy inbox files to the live inbox directory
2. Restore task bucket
3. Recompute highwatermark
4. Write `config.json`

If the process crashes between steps 1 and 4, inbox files for members not in config
exist with no detection mechanism.

### 19.2 Revised Restore Ordering (Config-Last with Staging)

```
1. Validate backup and compute restore plan (no mutations)
2. Write .restore-in-progress marker to team directory
3. Stage inbox files to .restore-staging/inboxes/
4. Move staged files to live inboxes/ (fs::rename — atomic same-filesystem)
5. Restore task bucket
6. Recompute highwatermark
7. Write config.json + fsync (atomic temp+rename via write_team_config)
8. Remove .restore-in-progress marker
```

Key properties:
- crash at steps 2-6: config.json unchanged, extra inbox files harmless, marker signals re-run
- crash at step 7: config write is itself atomic via the existing `write_team_config(...)`
  temp-file + rename path, so no partial config write is possible
- crash at step 8: config is written, stale marker cleaned up by next doctor/restore run

### 19.3 Staging Directory

- location: `{team_dir}/.restore-staging/inboxes/`
- lifecycle: created at step 3, contents moved at step 4, directory removed after config write
- failure path: staging directory cleaned up, no config written

### 19.4 Doctor Integration

New check: scan for `.restore-in-progress` in team directories.
- Severity: warning
- Recovery guidance: "A previous `atm teams restore` was interrupted. Re-run the restore
  command to complete it, or remove the marker file manually if the restore is no longer needed."

If `.restore-staging/` already exists at restore start, the implementation must
either clean it before staging begins or fail with actionable recovery text.
It must never merge old staging contents with the new restore attempt.

## 20. Phase M Minor Architecture Changes

### 20.1 AtmError Display Backtrace

`AtmError` keeps the user-facing `Display` output concise:

- `Display` renders only the primary message and recovery text
- captured backtraces stay available through Debug output and a dedicated
  accessor on `AtmError`

This avoids multi-kilobyte backtrace blobs in normal CLI/log output while
preserving full diagnostic depth for explicit debugging.

### 20.2 resolve_actor_identity Consolidation

Duplicate function in `ack/mod.rs`, `clear/mod.rs`, and `read/mod.rs` moves to
`identity/mod.rs` as `pub(crate) fn resolve_actor_identity(...)`. All three call sites
update to use the shared helper while preserving the existing override -> hook -> runtime
identity resolution order.

### 20.3 normalize_json_number Panic Removal

`normalize_json_number(...)` must not panic on untrusted numeric text. Phase M
replaces the old panic path with graceful fallback: on exponent parse failure or
unsupported exponent range, return the raw string unchanged and emit `tracing::warn!`.
A library function must not panic on potentially untrusted input.

### 20.4 Error-Surface Audit Methodology

Phase M uses an explicit audit methodology for `REQ-CORE-ERROR-DOC-001` and
`REQ-CORE-ERROR-RECOVERY-001` so signoff does not depend on ad hoc review.

Method:
- grep the production source tree for `expect(` and bare `AtmError`
  construction sites
- review the resulting inventory manually against the explicit Phase M audit
  inventory in the sprint plan
- exclude:
  - test-only code
  - `#[cfg(test)]` modules embedded in production files
  - intentional invariant assertions that do not represent operator-actionable
    failures
- keep the remaining production-path sites in scope for either:
  - `# Errors` documentation updates
  - `.with_recovery()` additions
  - panic removal or other structural correction when the failure mode is not
    acceptable in library code

The initial planning audit identified 16 production-path `expect(...)` sites
requiring review under this methodology. Phase M treats that number as a
starting inventory, not as a substitute for a fresh grep during implementation.

### 20.5 Phase L.7 Build-On Notes

Phase M builds on the already-landed L.7 runtime surface
(`team_members`, `aliases`, `post_send_hook`, doctor identity drift warning).
Phase M does not re-open that feature set; it only adds the remaining concurrency,
restore, and code-review hardening needed for 1.0.

### 20.6 Security Boundaries (Phase O)

Phase O adds three architecture-level hardening decisions:

1. **Address validation is the trust boundary for path construction**
   - team and agent names must be validated before any helper constructs
     `{ATM_HOME}/.claude/teams/{team}` or `{agent}.json`
   - `address.rs` and `home.rs` together form the boundary; downstream code
     must not attempt ad hoc sanitization after path joins are already built

2. **PID-file locking remains conservative by design**
   - the send-alert lock uses a PID-file-style stale-lock check
   - PID reuse is an accepted limitation: a reused PID can make a stale lock
     look alive, so ATM may conservatively preserve that stale lock until
     timeout or manual cleanup
   - this limitation favors false-alive availability loss over false-dead lock
     eviction

3. **Atomic writes must use collision-proof temp names**
   - temp files for atomic replacement must use UUID-based suffixes instead of
     timestamp-only suffixes
   - this keeps same-process rapid writes to the same target path from
     colliding on the temp-file name while preserving the target basename for
     operator debugging
