# Phase Y Delivery State Machines

## Goal

Make every write-affecting mail event auditable through one central
delivery-policy layer and one event-family state machine, rather than through
scattered command-local `if` branches.

Diagram viewer:

- [state-diagrams.md](./state-diagrams.md)
- [delivery-state-diagrams.html](../reports/delivery-state-diagrams.html)

## Core Rule

Delivery policy must be centralized, but the event machines must stay narrow.

- one central delivery-policy coordinator decides:
  - event family
  - harness family
  - allowed side effects
- each event family owns its own explicit state machine enum and transitions
- helper code may share side effects, but it must not collapse event legality
  into one generic “send-like” machine

This is explicitly **not** a “god object” that owns every mail state in one
enum.

## Current Gap

Today the repo has a first-class harness enum in durable roster state:

- `crates/atm-core/src/boundary/store.rs::RosterHarness`

But the write/nudge behavior is still spread across:

- `send`
- `ack`
- compatibility export
- post-send-hook execution

That is the policy leakage Phase `Y` must remove.

## Synchronization Minimization

Phase `Y` must remove or isolate broad application-level locking rather than
codify a new permanent lock hierarchy.

Required direction:

- roster resolution uses a short-lived snapshot read only
- SQLite transaction scope is the durable mutation boundary
- compatibility export / nudge side effects happen after the durable decision
  point
- no machine or coordinator design may require a new long-lived cross-domain
  lock spanning roster state, SQLite durability, and compatibility mailbox
  state

Explicit prohibitions:

- do not introduce a new coordinator-driven lock-ordering architecture
- do not hold live roster locks across SQLite I/O
- do not treat compatibility mailbox/workflow locking as a message-truth
  correctness boundary for the new daemon line
- do not widen synchronization just to smooth over edge cases around member
  add/remove timing

Transitional note:

- if a legacy compatibility-side lock still exists during `Y.3` / `Y.4`, the
  implementation must work to shrink or isolate that lock rather than make it a
  first-class planning primitive

## Required Central Coordinator

Phase `Y` must introduce one daemon-private or tightly daemon-owned
delivery-policy coordinator with these responsibilities only:

- accept an event-family request
- resolve the recipient harness from canonical roster truth
- dispatch into the correct event-family state machine
- emit observable transition records

The coordinator must not own all event logic internally. It is a dispatcher and
policy gate, not the universal state enum.

Harness-resolution rule:

- the coordinator resolves harness from canonical roster truth through the
  installed roster/store boundary
- it must not infer harness from:
  - model strings
  - ad hoc command flags
  - stale config-side compatibility fields

Synchronization rule:

- the coordinator must resolve roster state through a short-lived snapshot
  read, copy out the routing decision it needs, and release that read scope
  immediately
- the coordinator and machines must not introduce a broad application lock
  hierarchy spanning roster state, SQLite durability, and compatibility
  mailbox/workflow state
- the only approved execution shape is:
  - read canonical roster snapshot
  - perform the SQLite durability step
  - perform compatibility export / outward nudge side effects afterward
- any legacy mailbox/workflow lock that still exists during transition work is
  strictly a temporary compatibility-side implementation detail to be shrunk or
  isolated, not a new correctness boundary for message truth
- no implementation may hold a live roster lock across SQLite I/O
- no implementation may widen synchronization merely to smooth over
  add/remove-member race edges
- race handling should stay pragmatic:
  - membership changes around the edge of one message delivery are acceptable
    eventual-consistency cases
  - those cases must not justify long-lived roster locks or widened
    cross-subsystem locking

Required top-level enums:

```rust
enum DeliveryEventFamily {
    NewMessage,
    ThreadUpdate,
    AckReply,
    InboxRepair,
    RestoreInboxRebuild,
}

enum DeliveryHarnessPath {
    ClaudeCode,
    NonClaude,
}
```

## Coordinator Invariants

- restart recovery:
  - after daemon restart, the coordinator must rediscover in-flight work from
    durable SQLite truth or explicit retained repair queues; it must not assume
    in-memory delivery state survived process exit
- duplicate delivery guard:
  - every dispatchable delivery path must be keyed by durable message identity
  - rediscovery or retry must not create duplicate outward delivery for the
    same logical message without an explicit documented replay mode
- deferred-machine dispatch:
  - if a machine family is not yet landed, the coordinator must fail closed
    with a typed deferred-machine error instead of silently routing into an ad
    hoc compatibility path

## Concurrency Model

- the coordinator is constructed once at daemon init from shared read-only
  resources and boundary handles
- each delivery request constructs a short-lived per-request handle carrying
  copied routing facts and the exact machine dispatch target
- no coordinator-level lock may be held across:
  - SQLite writes
  - compatibility append/export I/O
  - nudge or post-send-hook execution
- implementers should prefer per-request state plus immutable shared config
  over `Arc<Mutex<DeliveryPolicyCoordinator>>` style coordination

## Typestate Design

Phase `Y` should encode key capability-transfer moments with typestate tokens
when `Y.4` implementation lands.

Required design tokens:

- `ValidatedDeliveryRequest`
  - created after event-family validation succeeds
  - proves the machine may advance into durable persistence
- `PersistedDeliveryRecord`
  - created after the SQLite durability step succeeds
  - proves post-durability side effects may begin
- `AckDeliveryToken`
  - created after ack-state validation/persistence succeeds
  - proves reply delegation into the new-message path is legal
- `RestoreMarkerToken`
  - created after restore-marker validation succeeds
  - proves restore rebuild may publish staged output

The design rule is:

- validation-before-persist and persist-before-dispatch should be represented
  by token handoff where feasible, not by prose-only sequencing

## Error Types

The implementation sprint must land typed error inventory that matches the
observable transition contract.

Preferred shape:

```rust
enum DeliveryError {
    NewMessage(NewMessageError),
    ThreadUpdate(ThreadUpdateError),
    AckReply(AckReplyError),
    InboxRepair(InboxRepairError),
    RestoreInboxRebuild(RestoreInboxRebuildError),
    DeferredMachine(DeferredMachineError),
}
```

Required per-family error coverage:

- `NewMessageError`
  - `RosterLookupFailed`
  - `SqlitePersistFailed`
  - `CompatibilityAppendFailed`
  - `CompanionErrorEmitFailed`
  - `PrimaryNudgeFailed`
  - `ErrorNudgeFailed`
- `ThreadUpdateError`
  - `ParentMissing`
  - `RootMissing`
  - `SenderMismatch`
  - `LinearityRejected`
  - `SqlitePersistFailed`
- `AckReplyError`
  - `AckTargetMissing`
  - `ReplyTargetRejected`
  - `AckStatePersistFailed`
  - `ReplyDelegationFailed`
- `InboxRepairError`
  - `HarnessRejected`
  - `ProjectionLoadFailed`
  - `StageFailed`
  - `PublishFailed`
- `RestoreInboxRebuildError`
  - `RestoreMarkerRejected`
  - `HarnessRejected`
  - `ProjectionLoadFailed`
  - `StageFailed`
  - `PublishFailed`

Observable transition rule:

- every failure or degradation transition must map to one named error variant
- every such transition must emit a stable `error_code`

## Required Event-Family State Machines

### 1. New Message

Required top-level machine:

- `NewMessageStateMachine`

Required audited harness paths:

- `ClaudeHarnessNewMessage`
- `NonClaudeHarnessNewMessage`

Required machine enums:

```rust
enum NewMessageCoordinatorState {
    Received,
    ResolveHarness,
    DispatchClaude,
    DispatchNonClaude,
    Completed,
    Rejected,
}

enum ClaudeHarnessNewMessageState {
    Received,
    PersistSqlite,
    SqliteCommitted,
    AppendCompatibilityMessage,
    AppendCompatibilityErrorMessage,
    RunPrimaryNudge,
    RunErrorNudge,
    RunPostSendHookFallback,
    Delivered,
    Failed,
}

enum NonClaudeHarnessNewMessageState {
    Received,
    PersistSqlite,
    SqliteCommitted,
    DeliverOriginal,
    DeliverErrorMessage,
    RunPrimaryNudge,
    RunErrorNudge,
    Delivered,
    Failed,
}
```

Why it is separate:

- no parent thread precondition is required
- the event is fundamentally “deliver one new logical message”
- harness-dependent output behavior is significant

#### Coordinator Transition Table

| From | Trigger | Preconditions | To | Observable transition |
|---|---|---|---|---|
| `Received` | `start` | event family is `NewMessage` | `ResolveHarness` | `delivery_policy.new_message.received` |
| `ResolveHarness` | `harness=ClaudeCode` | canonical roster member exists | `DispatchClaude` | `delivery_policy.new_message.harness_claude` |
| `ResolveHarness` | `harness!=ClaudeCode` | canonical roster member exists | `DispatchNonClaude` | `delivery_policy.new_message.harness_non_claude` |
| `ResolveHarness` | `roster lookup failed` | none | `Rejected` | `delivery_policy.new_message.rejected` |
| `DispatchClaude` | `machine returned terminal` | none | `Completed` | `delivery_policy.new_message.completed` |
| `DispatchNonClaude` | `machine returned terminal` | none | `Completed` | `delivery_policy.new_message.completed` |

Coordinator execution contract:

- `ResolveHarness` uses a canonical roster snapshot and must not hold that
  snapshot read scope across later SQLite persistence or compatibility export
  side effects
- the coordinator may pass copied routing facts into the machine, but not live
  lock guards or mutable roster handles

#### Claude Harness Transition Table

| From | Trigger | Preconditions | To | Side effects | Observable transition |
|---|---|---|---|---|---|
| `Received` | `begin` | none | `PersistSqlite` | none | `new_message.claude.received` |
| `PersistSqlite` | `sqlite_ok` | durable write succeeded | `SqliteCommitted` | persist message + state | `new_message.claude.sqlite_committed` |
| `PersistSqlite` | `sqlite_err` | durable write failed | `AppendCompatibilityMessage` | retain sqlite failure diagnostics | `new_message.claude.sqlite_failed` |
| `SqliteCommitted` | `continue` | none | `AppendCompatibilityMessage` | none | `new_message.claude.append_original_start` |
| `AppendCompatibilityMessage` | `append_ok_after_sqlite_ok` | original message append succeeded | `RunPrimaryNudge` | append original message | `new_message.claude.append_original_ok` |
| `AppendCompatibilityMessage` | `append_err_after_sqlite_ok` | original message append failed | `RunPostSendHookFallback` | record append failure | `new_message.claude.append_original_failed` |
| `AppendCompatibilityMessage` | `append_ok_after_sqlite_err` | original message append succeeded after sqlite failure | `AppendCompatibilityErrorMessage` | append original message | `new_message.claude.append_original_after_sqlite_failure_ok` |
| `AppendCompatibilityMessage` | `append_err_after_sqlite_err` | original message append failed after sqlite failure | `Failed` | record blocking failure | `new_message.claude.append_original_after_sqlite_failure_failed` |
| `AppendCompatibilityErrorMessage` | `append_ok` | sqlite error companion append succeeded | `RunPrimaryNudge` | append `atm-system@<team>` error message | `new_message.claude.append_error_ok` |
| `AppendCompatibilityErrorMessage` | `append_err` | sqlite error companion append failed | `Failed` | emit `ERR_COMPANION_EMIT_FAILED`; record blocking failure | `new_message.claude.append_error_failed` |
| `RunPrimaryNudge` | `sqlite_ok_path` | no companion error required | `Delivered` | run original nudge | `new_message.claude.nudge_original_ok` |
| `RunPrimaryNudge` | `sqlite_err_path` | companion error path active | `RunErrorNudge` | run original nudge | `new_message.claude.nudge_original_after_sqlite_failure_ok` |
| `RunPrimaryNudge` | `nudge_err` | original nudge failed | `Failed` | emit `ERR_PRIMARY_NUDGE_FAILED`; record notification failure | `new_message.claude.nudge_original_failed` |
| `RunErrorNudge` | `nudge_ok` | companion error path active | `Delivered` | run error nudge | `new_message.claude.nudge_error_ok` |
| `RunErrorNudge` | `nudge_err` | companion error nudge failed | `Failed` | emit `ERR_ERROR_NUDGE_FAILED`; record notification failure | `new_message.claude.nudge_error_failed` |
| `RunPostSendHookFallback` | `hook_ok_or_warn` | sqlite committed earlier | `Delivered` | execute post-send-hook fallback | `new_message.claude.hook_fallback_completed` |

Lock-minimization notes:

- `PersistSqlite` is the durable mutation boundary
- `AppendCompatibilityMessage`, `AppendCompatibilityErrorMessage`, and nudge
  states are post-durability side effects
- these side effects must not rely on a long-lived roster lock or broaden
  SQLite transaction scope just to keep compatibility output “in sync”

#### Non-Claude Harness Transition Table

| From | Trigger | Preconditions | To | Side effects | Observable transition |
|---|---|---|---|---|---|
| `Received` | `begin` | none | `PersistSqlite` | none | `new_message.non_claude.received` |
| `PersistSqlite` | `sqlite_ok` | durable write succeeded | `SqliteCommitted` | persist message + state | `new_message.non_claude.sqlite_committed` |
| `PersistSqlite` | `sqlite_err` | durable write failed | `DeliverOriginal` | retain sqlite failure diagnostics | `new_message.non_claude.sqlite_failed` |
| `SqliteCommitted` | `continue` | none | `DeliverOriginal` | none | `new_message.non_claude.deliver_original_start` |
| `DeliverOriginal` | `deliver_ok_after_sqlite_ok` | original non-Claude delivery succeeded | `RunPrimaryNudge` | deliver original through non-Claude path | `new_message.non_claude.deliver_original_ok` |
| `DeliverOriginal` | `deliver_ok_after_sqlite_err` | original non-Claude delivery succeeded after sqlite failure | `DeliverErrorMessage` | deliver original through non-Claude path | `new_message.non_claude.deliver_original_after_sqlite_failure_ok` |
| `DeliverOriginal` | `deliver_err` | original non-Claude delivery failed | `Failed` | record delivery failure | `new_message.non_claude.deliver_original_failed` |
| `DeliverErrorMessage` | `deliver_ok` | companion error delivery succeeded | `RunPrimaryNudge` | deliver `atm-system@<team>` error through non-Claude path | `new_message.non_claude.deliver_error_ok` |
| `DeliverErrorMessage` | `deliver_err` | companion error delivery failed | `Failed` | emit `ERR_COMPANION_EMIT_FAILED`; record blocking failure | `new_message.non_claude.deliver_error_failed` |
| `RunPrimaryNudge` | `sqlite_ok_path` | no companion error required | `Delivered` | run original nudge | `new_message.non_claude.nudge_original_ok` |
| `RunPrimaryNudge` | `sqlite_err_path` | companion error path active | `RunErrorNudge` | run original nudge | `new_message.non_claude.nudge_original_after_sqlite_failure_ok` |
| `RunPrimaryNudge` | `nudge_err` | original nudge failed | `Failed` | emit `ERR_PRIMARY_NUDGE_FAILED`; record notification failure | `new_message.non_claude.nudge_original_failed` |
| `RunErrorNudge` | `nudge_ok` | companion error path active | `Delivered` | run error nudge | `new_message.non_claude.nudge_error_ok` |
| `RunErrorNudge` | `nudge_err` | companion error nudge failed | `Failed` | emit `ERR_ERROR_NUDGE_FAILED`; record notification failure | `new_message.non_claude.nudge_error_failed` |

Lock-minimization notes:

- non-Claude delivery follows the same snapshot -> SQLite -> outward-side-effect
  model
- non-Claude paths must not inherit Claude-compatibility mailbox locking
  concerns at all

### 2. Thread Update

Required top-level machine:

- `ThreadUpdateStateMachine`

Required audited modes:

- `add-details`
- `supersede`

Required machine enums:

```rust
enum ThreadUpdateCoordinatorState {
    Received,
    ResolveHarness,
    ValidateUpdateLegality,
    DispatchUpdateDelivery,
    Completed,
    Rejected,
}

enum ThreadUpdateMode {
    AddDetails,
    Supersede,
}

enum ThreadUpdateState {
    Received,
    ValidateParentExists,
    ValidateRootExists,
    ValidateOriginalSender,
    ValidateLinearSuccessor,
    PersistSqlite,
    DispatchByHarness,
    Delivered,
    Rejected,
    Failed,
}
```

Why it is separate from `NewMessageStateMachine`:

- it has different legality rules:
  - parent must exist
  - root must resolve
  - original-sender identity must match
  - one-successor / linear-thread constraints must hold
- the side effects differ from standalone send
- QA needs a separate transition table for update legality and failure modes

#### Thread Update Transition Table

| From | Trigger | Preconditions | To | Side effects | Observable transition |
|---|---|---|---|---|---|
| `Received` | `begin` | mode is `add-details` or `supersede` | `ValidateParentExists` | none | `thread_update.received` |
| `ValidateParentExists` | `parent_found` | referenced parent exists | `ValidateRootExists` | none | `thread_update.parent_ok` |
| `ValidateParentExists` | `parent_missing` | none | `Rejected` | emit validation error | `thread_update.parent_missing` |
| `ValidateRootExists` | `root_found` | thread root resolves | `ValidateOriginalSender` | none | `thread_update.root_ok` |
| `ValidateRootExists` | `root_missing` | none | `Rejected` | emit validation error | `thread_update.root_missing` |
| `ValidateOriginalSender` | `sender_matches` | canonical sender matches root sender | `ValidateLinearSuccessor` | none | `thread_update.sender_ok` |
| `ValidateOriginalSender` | `sender_mismatch` | none | `Rejected` | emit validation error | `thread_update.sender_mismatch` |
| `ValidateLinearSuccessor` | `successor_clear` | no successor already exists | `PersistSqlite` | none | `thread_update.linearity_ok` |
| `ValidateLinearSuccessor` | `successor_exists` | none | `Rejected` | emit validation error | `thread_update.linearity_rejected` |
| `PersistSqlite` | `sqlite_ok` | durable write succeeded | `DispatchByHarness` | persist new thread-update row/state | `thread_update.sqlite_committed` |
| `PersistSqlite` | `sqlite_err` | durable write failed | `Failed` | record persistence failure | `thread_update.sqlite_failed` |
| `DispatchByHarness` | `claude_or_non_claude_terminal` | harness resolved and side effects complete | `Delivered` | dispatch through harness-specific delivery effects | `thread_update.delivered` |

### 3. Ack Reply

Required top-level machine:

- `AckReplyStateMachine`

Required machine enums:

```rust
enum AckReplyCoordinatorState {
    Received,
    ValidateAckTarget,
    ResolveHarness,
    DispatchAckReply,
    Completed,
    Rejected,
}

enum AckReplyState {
    Received,
    ValidateAckTargetExists,
    ValidateReplyTargetAllowed,
    PersistAckTransition,
    BuildReplyDeliveryRequest,
    DispatchReplyByHarness,
    Delivered,
    Rejected,
    Failed,
}
```

Why it is separate:

- ack legality is not the same as either standalone send or thread update
- reply delivery must inherit the approved `NewMessage` harness contract without
  re-embedding that logic in `ack`
- the machine must make the ack transition observable before the reply leaves
  the coordinator

#### Ack Reply Transition Table

| From | Trigger | Preconditions | To | Side effects | Observable transition |
|---|---|---|---|---|---|
| `Received` | `begin` | reply mode is `ack` | `ValidateAckTargetExists` | none | `ack_reply.received` |
| `ValidateAckTargetExists` | `target_found` | referenced message exists | `ValidateReplyTargetAllowed` | none | `ack_reply.target_ok` |
| `ValidateAckTargetExists` | `target_missing` | none | `Rejected` | emit validation error | `ack_reply.target_missing` |
| `ValidateReplyTargetAllowed` | `reply_allowed` | reply target and sender policy pass | `PersistAckTransition` | none | `ack_reply.reply_allowed` |
| `ValidateReplyTargetAllowed` | `reply_rejected` | none | `Rejected` | emit validation error | `ack_reply.reply_rejected` |
| `PersistAckTransition` | `sqlite_ok` | ack-state write succeeded | `BuildReplyDeliveryRequest` | persist ack transition | `ack_reply.ack_state_committed` |
| `PersistAckTransition` | `sqlite_err` | none | `Failed` | record blocking persistence failure | `ack_reply.ack_state_failed` |
| `BuildReplyDeliveryRequest` | `reply_ready` | reply payload and recipient resolved | `DispatchReplyByHarness` | construct reply delivery request | `ack_reply.reply_ready` |
| `DispatchReplyByHarness` | `delegate_to_new_message_machine` | harness resolved | `Delivered` | invoke the approved `NewMessage` harness path verbatim for the reply payload | `ack_reply.delivered` |

### 4. Inbox Repair

Required top-level machine:

- `InboxRepairStateMachine`

Required machine enums:

```rust
enum InboxRepairCoordinatorState {
    Received,
    ResolveHarness,
    ValidateRepairRequest,
    DispatchRepair,
    Completed,
    Rejected,
}

enum InboxRepairState {
    Received,
    ValidateClaudeHarness,
    LoadRepairProjection,
    FilterDeletedMessages,
    StageInboxRebuild,
    PublishInboxRebuild,
    Delivered,
    Rejected,
    Failed,
}
```

Why it is separate:

- this is the explicit bulk mailbox creation / rebuild path
- it is allowed to stage and publish a bounded historical projection
- it must stay distinct from normal runtime message delivery and from restore

#### Inbox Repair Transition Table

| From | Trigger | Preconditions | To | Side effects | Observable transition |
|---|---|---|---|---|---|
| `Received` | `begin` | explicit repair/rebuild request accepted | `ValidateClaudeHarness` | none | `inbox_repair.received` |
| `ValidateClaudeHarness` | `harness=ClaudeCode` | canonical roster member exists | `LoadRepairProjection` | none | `inbox_repair.harness_ok` |
| `ValidateClaudeHarness` | `harness!=ClaudeCode` | none | `Rejected` | emit validation error | `inbox_repair.harness_rejected` |
| `LoadRepairProjection` | `projection_loaded` | bounded source projection available | `FilterDeletedMessages` | load rebuild candidate set | `inbox_repair.projection_loaded` |
| `LoadRepairProjection` | `projection_err` | none | `Failed` | record projection failure | `inbox_repair.projection_failed` |
| `FilterDeletedMessages` | `filter_complete` | deleted messages excluded | `StageInboxRebuild` | produce non-deleted rebuild set | `inbox_repair.filter_complete` |
| `StageInboxRebuild` | `stage_ok` | staged inbox image prepared | `PublishInboxRebuild` | create temp/staged inbox output | `inbox_repair.staged` |
| `StageInboxRebuild` | `stage_err` | none | `Failed` | delete staged temp file; record staging failure | `inbox_repair.stage_failed` |
| `PublishInboxRebuild` | `publish_ok` | staged output atomically published | `Delivered` | create or replace the repaired Claude inbox | `inbox_repair.published` |
| `PublishInboxRebuild` | `publish_err` | none | `Failed` | delete staged temp file; record publish failure | `inbox_repair.publish_failed` |

### 5. Restore Inbox Rebuild

Required top-level machine:

- `RestoreInboxRebuildStateMachine`

Required machine enums:

```rust
enum RestoreInboxRebuildCoordinatorState {
    Received,
    ValidateRestoreScope,
    ResolveHarness,
    DispatchRestoreRebuild,
    Completed,
    Rejected,
}

enum RestoreInboxRebuildState {
    Received,
    ValidateRestoreMarker,
    ValidateClaudeHarness,
    LoadRestoreProjection,
    StageRestoreOutput,
    PublishRestoreOutput,
    Delivered,
    Rejected,
    Failed,
}
```

Why it is separate:

- restore is a privileged recovery path with its own staging, rollback, and
  restore-marker semantics
- it must not be conflated with either normal runtime delivery or ad hoc inbox
  repair
- QA must be able to audit restore-specific publish and cleanup transitions

#### Restore Inbox Rebuild Transition Table

| From | Trigger | Preconditions | To | Side effects | Observable transition |
|---|---|---|---|---|---|
| `Received` | `begin` | explicit restore/rebuild request accepted | `ValidateRestoreMarker` | none | `restore_inbox.rebuild_received` |
| `ValidateRestoreMarker` | `marker_ok` | restore marker / staging contract is valid | `ValidateClaudeHarness` | none | `restore_inbox.marker_ok` |
| `ValidateRestoreMarker` | `marker_missing_or_invalid` | none | `Rejected` | emit validation error | `restore_inbox.marker_rejected` |
| `ValidateClaudeHarness` | `harness=ClaudeCode` | canonical roster member exists | `LoadRestoreProjection` | none | `restore_inbox.harness_ok` |
| `ValidateClaudeHarness` | `harness!=ClaudeCode` | none | `Rejected` | emit validation error | `restore_inbox.harness_rejected` |
| `LoadRestoreProjection` | `projection_loaded` | restore source set available | `StageRestoreOutput` | load restore projection | `restore_inbox.projection_loaded` |
| `LoadRestoreProjection` | `projection_err` | none | `Failed` | record projection failure | `restore_inbox.projection_failed` |
| `StageRestoreOutput` | `stage_ok` | restore output staged successfully | `PublishRestoreOutput` | write staged rebuild output | `restore_inbox.staged` |
| `StageRestoreOutput` | `stage_err` | none | `Failed` | delete staged output; remove restore marker; record staging failure | `restore_inbox.stage_failed` |
| `PublishRestoreOutput` | `publish_ok` | staged restore output atomically published | `Delivered` | publish rebuilt inbox and clear restore staging as documented | `restore_inbox.published` |
| `PublishRestoreOutput` | `publish_err` | none | `Failed` | delete staged output; remove restore marker; record publish failure | `restore_inbox.publish_failed` |

### 6. Deferral Rule

If any required family above is not landed in the same sprint as the central
coordinator, the plan must say:

- why it is deferred
- which existing path still owns it temporarily
- which sprint will absorb it

## Shared Side-Effect Helpers

Helper layers may be shared for:

- SQLite message persistence
- SQLite message-state persistence
- compatibility append/export
- post-send-hook fallback execution
- observability emission

But the helper layer must not decide:

- whether this is a new message or a thread update
- whether a given update is legal
- whether a harness may use JSONL
- whether a companion error message is required

Those decisions belong to the event-family state machine plus the central
delivery-policy coordinator.

## Observable Transition Contract

Every write-affecting transition must emit one structured transition record.

Minimum required fields:

- `event_family`
- `machine`
- `from_state`
- `to_state`
- `trigger`
- `team`
- `agent`
- `harness`
- `message_id` when one exists
- `task_id` when one exists
- `result`
- `error_code` when the transition represents failure or degradation

Naming rule:

- observability event names should stay stable and machine-oriented, for
  example:
  - `new_message.claude.sqlite_committed`
  - `thread_update.sender_mismatch`
- QA should verify transition coverage by these stable event names rather than
  by ad hoc log text

## Harness Rules

Harness selection is based on canonical roster `harness`, not model.

- `Claude Code` harness may use the compatibility JSONL append path
- non-`Claude Code` harnesses must never use the compatibility JSONL append
  path
- the model (`opus`, `haiku`, `sonnet`, etc.) is irrelevant to this branch

`nudge` definition:

- `nudge` means the harness-specific notification side effect that occurs after
  the message path for that harness
- for `Claude Code`, that includes the compatibility append-driven user-visible
  wake-up behavior
- for non-Claude harnesses, that means the harness-native notification path
- the state machines should model the nudge abstractly; adapter-specific
  mechanics stay below the machine line

## New Message Runtime Contract

### Claude Harness

Success path:

1. durable SQLite message/state write completes
2. owned Claude-compatible append path runs
3. normal nudge path runs

SQLite failure path:

1. outward delivery still proceeds
2. ATM appends the original message to the Claude Code inbox
3. ATM appends a second error message from `atm-system@<team>` to the Claude
   Code inbox
4. the nudge path mirrors both messages

Append failure path:

1. message/outward-delivery truth is not reinterpreted
2. fallback notification path is post-send-hook execution
3. no alternate fallback path is introduced

### Non-Claude Harness

Success path:

1. durable SQLite message/state write completes
2. no JSONL append occurs
3. the non-Claude outward delivery / notification path runs

SQLite failure path:

1. outward delivery still proceeds
2. ATM emits the original message through the non-Claude delivery path
3. ATM emits a second error message from `atm-system@<team>` through the same
   non-Claude delivery path
4. the nudge/notification path mirrors both messages

## Trade-Off: One Machine Vs Several

Why Phase `Y` must use separate event-family machines:

- one combined machine would mix:
  - new-message delivery
  - thread legality
  - supersede/update semantics
  - harness routing
  - error companion policy
- that would create many invalid states and reintroduce branch-heavy logic

Why a central coordinator is still required:

- harness selection and event-family dispatch must live somewhere explicit
- QA needs one place to verify that no hidden path bypasses the approved
  machines

Rule of thumb:

- share side effects
- do not share event legality

## Required QA Artifacts

QA must have one transition table per required machine:

- `ClaudeHarnessNewMessage`
- `NonClaudeHarnessNewMessage`
- `ThreadUpdateStateMachine`
- `AckReplyStateMachine`
- `InboxRepairStateMachine`
- `RestoreInboxRebuildStateMachine`

Each table must list:

- starting state
- trigger
- preconditions
- allowed side effects
- terminal state
- observable transition record

Phase `Y` does not close until those tables exist and QA verifies that the
implementation matches them.
