# Phase Y Delivery State Machines

## Goal

Make every write-affecting mail event auditable through one central
delivery-policy layer and one event-family state machine, rather than through
scattered command-local `if` branches.

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
| `AppendCompatibilityErrorMessage` | `append_err` | sqlite error companion append failed | `Failed` | record blocking failure | `new_message.claude.append_error_failed` |
| `RunPrimaryNudge` | `sqlite_ok_path` | no companion error required | `Delivered` | run original nudge | `new_message.claude.nudge_original_ok` |
| `RunPrimaryNudge` | `sqlite_err_path` | companion error path active | `RunErrorNudge` | run original nudge | `new_message.claude.nudge_original_after_sqlite_failure_ok` |
| `RunErrorNudge` | `nudge_ok` | companion error path active | `Delivered` | run error nudge | `new_message.claude.nudge_error_ok` |
| `RunPostSendHookFallback` | `hook_ok_or_warn` | sqlite committed earlier | `Delivered` | execute post-send-hook fallback | `new_message.claude.hook_fallback_completed` |

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
| `DeliverErrorMessage` | `deliver_err` | companion error delivery failed | `Failed` | record blocking failure | `new_message.non_claude.deliver_error_failed` |
| `RunPrimaryNudge` | `sqlite_ok_path` | no companion error required | `Delivered` | run original nudge | `new_message.non_claude.nudge_original_ok` |
| `RunPrimaryNudge` | `sqlite_err_path` | companion error path active | `RunErrorNudge` | run original nudge | `new_message.non_claude.nudge_original_after_sqlite_failure_ok` |
| `RunErrorNudge` | `nudge_ok` | companion error path active | `Delivered` | run error nudge | `new_message.non_claude.nudge_error_ok` |

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

### 3. Follow-On Families

These may land after the first two, but they must not be smuggled back into
generic command logic:

- `AckReplyStateMachine`
- `InboxRepairStateMachine`
- `RestoreInboxRebuildStateMachine`

Deferral rule:

- if any follow-on family is not landed in the same sprint as the central
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

Each table must list:

- starting state
- trigger
- preconditions
- allowed side effects
- terminal state
- observable transition record

Phase `Y` does not close until those tables exist and QA verifies that the
implementation matches them.
