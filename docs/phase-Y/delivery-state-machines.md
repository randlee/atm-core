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

## Required Event-Family State Machines

### 1. New Message

Required top-level machine:

- `NewMessageStateMachine`

Required audited harness paths:

- `ClaudeHarnessNewMessage`
- `NonClaudeHarnessNewMessage`

Why it is separate:

- no parent thread precondition is required
- the event is fundamentally “deliver one new logical message”
- harness-dependent output behavior is significant

### 2. Thread Update

Required top-level machine:

- `ThreadUpdateStateMachine`

Required audited modes:

- `add-details`
- `supersede`

Why it is separate from `NewMessageStateMachine`:

- it has different legality rules:
  - parent must exist
  - root must resolve
  - original-sender identity must match
  - one-successor / linear-thread constraints must hold
- the side effects differ from standalone send
- QA needs a separate transition table for update legality and failure modes

### 3. Follow-On Families

These may land after the first two, but they must not be smuggled back into
generic command logic:

- `AckReplyStateMachine`
- `InboxRepairStateMachine`
- `RestoreInboxRebuildStateMachine`

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

## Harness Rules

Harness selection is based on canonical roster `harness`, not model.

- `Claude Code` harness may use the compatibility JSONL append path
- non-`Claude Code` harnesses must never use the compatibility JSONL append
  path
- the model (`opus`, `haiku`, `sonnet`, etc.) is irrelevant to this branch

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
