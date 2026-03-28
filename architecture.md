# ATM CLI Architecture

## 1. Overview

The rewrite keeps ATM as a file-based mail CLI and removes daemon architecture.

The workspace remains intentionally small:
- `atm-core`: reusable library
- `atm`: CLI binary

The CLI stays thin. Product logic moves into `atm-core`.

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
- structured error types
- observability integration

`atm-core` must not depend on clap or terminal formatting concerns.

### 2.2 `atm`

`atm` owns:
- clap argument parsing
- command dispatch
- output rendering
- process exit behavior
- one-time observability initialization

`atm` must not implement mailbox, config, or workflow business logic directly.

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
  error.rs
  home.rs
  identity/
    hook.rs
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
    mod.rs
    read.rs
    send.rs
  output.rs
```

Notes:
- no plugin framework
- no daemon client
- no runtime spawning layer
- no separate team store module in the MVP; direct team config loading is enough

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

`ReadState` remains a canonical classification target for legacy and informational messages, even though the normal MVP read path does not create it.

The public `MessageState` enum is for reporting and filtering. The typestate markers enforce legal transitions inside `atm-core`.

## 5. Persisted Schema

### 5.1 Team Config

The rewrite reuses the existing team config schema where feasible.

Only a small subset is required by send/read:
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

`send::send_mail(request: SendRequest) -> Result<SendOutcome, AtmError>`

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
- resolved team
- resolved recipient
- resolved sender
- generated message id
- summary
- rendered message body
- delivery result

The file-reference path may be rewritten through the file policy layer.

### 6.2 Read Service

Public entrypoint:

`read::read_mail(query: ReadQuery) -> Result<ReadOutcome, AtmError>`

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

`ReadOutcome` contains:
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
- `messages`
- `count`
- `bucket_counts`
- `history_collapsed`

## 7. Read Pipeline

The read pipeline stages are:
1. resolve actor
2. resolve target inbox
3. build the hostname registry for configured origin inboxes
4. load mailbox records from the merged inbox surface
5. classify workflow state
6. apply sender and timestamp filters
7. apply seen-state filter unless selection is `All`
8. map workflow state to display bucket and apply selection mode
9. sort newest-first
10. apply limit
11. apply legal read transitions for displayed unread messages
12. persist state changes atomically
13. update seen-state when enabled
14. return outcome

This ordering is part of the architecture contract.

## 8. Mailbox Storage

The mailbox layer owns:
- tolerant reads
- atomic append
- duplicate suppression
- conflict merge
- origin-inbox merge
- atomic workflow-state updates

The mailbox layer does not own selection policy, display buckets, or output formatting.

## 9. Identity And File Policy

### 9.1 Hook Identity

Hook-file identity is retained because it is a current non-daemon convenience path for send/read identity resolution.

Only hook identity resolution is required for the rewrite. Session-resolution paths that exist only to bridge runtime/daemon ambiguity are not required.

### 9.2 File Policy

The current `send --file` behavior is retained:
- inspect Claude settings permissions when available
- if the referenced file is allowed, send a direct file reference
- otherwise copy to ATM share storage and rewrite the message body accordingly

## 10. Observability

`atm-core::observability` adapts ATM domain events into `sc-observability`.

Initialization:
- `atm` initializes logging once at process startup
- `atm-core` exposes a small emit API for command lifecycle and mailbox skip events
- logging failures degrade to no-op behavior

Required event classes:
- command start
- command success
- command failure
- mailbox record skipped

Required event fields:
- command
- team
- actor
- target
- outcome
- error class when applicable
- message count when applicable
- transition count when applicable

## 11. Error Model

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

Every public error must include:
- a stable class
- human-readable cause
- recovery guidance when the user can act

## 12. Trait Policy

The MVP should avoid public extension traits.

If a trait becomes necessary:
- prefer a sealed trait
- verify object safety before stabilization

## 13. Testing Strategy

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

`atm` tests:
- clap parsing
- JSON output shape
- human-readable output snapshots
- send/read integration behavior
