# ATM Read Behavior Review

This document defines the canonical read behavior for the rewrite and records which parts of current ATM behavior are preserved.

## 1. Why This Document Exists

The current read path mixes several concerns:
- mail visibility
- workflow state
- display buckets
- watermark tracking
- wait-mode behavior

That makes the command hard to reason about and easy to regress.

The rewrite keeps the useful read behavior but makes the model explicit:
- canonical message state
- display bucket mapping
- selection policy
- legal state transitions

## 2. Best Current ATM Behavior To Preserve

The current command already has useful queue behavior that should survive the rewrite:
- default view shows actionable work only
- pending-ack messages stay visible until they are acknowledged
- history can be expanded without hiding actionable work
- `--all` shows everything
- older unread messages remain visible even when the seen-state watermark is newer
- last-seen updates from the latest displayed message, not the latest message in the inbox
- reading another agent’s inbox does not mutate it

The rewrite should preserve those behaviors while removing daemon coupling and making the workflow explicit in code.

## 3. Current Read Contract

### 3.1 Display Buckets

The current command exposes three display buckets:
- unread
- pending ack
- history

Current default output:
- show unread bucket
- show pending-ack bucket
- collapse history to a count line

Current flag behavior:
- default => actionable queue only
- `--unread-only` => unread bucket only
- `--pending-ack-only` => pending-ack bucket only
- `--history` => actionable queue plus history bucket
- `--all` => everything

### 3.2 Watermark Behavior

Current tests establish these rules:
- seen-state filtering is on by default
- unread messages remain visible even when older than the watermark
- pending-ack messages remain visible even when older than the watermark
- pure history can be hidden by the watermark
- `--all` bypasses the seen-state filter
- first run still shows only pending-action messages by default

### 3.3 Current Mutation Behavior

Current behavior when reading your own inbox with marking enabled:
- displayed unread messages are marked `read = true`
- displayed unread messages also receive `pendingAckAt`

Current ack behavior:
- an acknowledged message receives `acknowledgedAt`
- `pendingAckAt` is removed
- a reply message is emitted and references the original message id

This behavior is messy in the current code because the state machine is implicit. The rewrite keeps the behavior but makes the state model explicit.

## 4. Canonical Workflow State Model

The retained workflow has four canonical states:
- `Unread`
- `PendingAck`
- `Acknowledged`
- `Read`

Classification rule:
- `acknowledgedAt` present => `Acknowledged`
- else `pendingAckAt` present => `PendingAck`
- else `read = false` => `Unread`
- else `read = true` => `Read`

Important distinction:
- canonical workflow state is the domain model
- display buckets are the CLI presentation model

## 5. Display Bucket Mapping

Display bucket mapping from canonical state:
- `Unread` => `unread`
- `PendingAck` => `pending_ack`
- `Acknowledged` => `history`
- `Read` => `history`

That means:
- history is a bucket, not a state
- `Acknowledged` and `Read` must remain distinct in the state model even though both render into history

## 6. Legal Workflow Transitions

```text
New message persisted
  -> Unread

Read own inbox, marking enabled
  Unread -> PendingAck

Read own inbox, --no-mark
  Unread -> Unread

Read other inbox
  Unread -> Unread
  PendingAck -> PendingAck
  Acknowledged -> Acknowledged
  Read -> Read

Ack workflow
  PendingAck -> Acknowledged
```

Disallowed transitions:
- `Read -> Unread`
- `Acknowledged -> PendingAck`
- `Acknowledged -> Read`
- any transition that skips the legal graph

Notes:
- the normal read path does not create `Read` directly
- `Read` still exists as a canonical state because legacy data and informational messages may already be `read = true` without pending-ack fields

## 7. Seen-State Rules

Seen-state is a selection policy, not a state transition.

Rules:
- enable it by default
- disable it with `--no-since-last-seen`
- bypass it with `--all`
- keep older unread messages visible
- keep older pending-ack messages visible
- allow the watermark to hide only history items

Watermark update rule:
- update from the latest displayed message only
- do not include filtered-out messages

## 8. Required API Shape

The core read pipeline must encode the transition rules in types.

Minimum shape:

```rust
pub enum MessageState {
    Unread,
    PendingAck,
    Acknowledged,
    Read,
}

pub enum DisplayBucket {
    Unread,
    PendingAck,
    History,
}

pub struct StoredMessage<S> {
    // persisted fields
    // state marker
}

pub struct UnreadState;
pub struct PendingAckState;
pub struct AcknowledgedState;
pub struct ReadState;

impl StoredMessage<UnreadState> {
    pub fn mark_pending_ack(self, at: IsoTimestamp) -> StoredMessage<PendingAckState>;
}

impl StoredMessage<PendingAckState> {
    pub fn acknowledge(self, at: IsoTimestamp) -> StoredMessage<AcknowledgedState>;
}
```

Classification boundary:
- wire schema -> stateful core model

Rendering boundary:
- stateful core model -> CLI display buckets and rows

## 9. Recommended Read Algorithm

1. Resolve actor identity.
2. Resolve target inbox.
3. Load the merged inbox surface.
4. Convert wire records into canonical stateful messages.
5. Apply sender and timestamp filters.
6. Apply seen-state filtering unless selection is `All`.
7. Apply selection mode.
8. Sort newest-first.
9. Apply limit.
10. Persist legal workflow transitions for displayed unread messages if allowed.
11. Update seen-state from the displayed set when enabled.
12. Return `ReadOutcome`.

This order matters.

In particular:
- selection must happen before mutation
- mutation must happen before final output is returned
- seen-state updates must use the displayed set, not the full inbox

## 10. Output Contract

Human output:
- queue heading
- bucket counts line
- unread bucket
- pending-ack bucket
- optional history bucket
- hidden-history line when history is collapsed

JSON output:
- selected messages only
- `count`
- `bucket_counts`
- `history_collapsed`

`bucket_counts` fields:
- `unread`
- `pending_ack`
- `history`

## 11. Review Standard

An implementation of `atm read` is acceptable only if:
- it uses the canonical four-state workflow model
- it keeps display buckets separate from workflow state
- it preserves default actionable-queue behavior
- it preserves the current pending-ack lifecycle
- no daemon-only logic survives in core read behavior
- unread-to-pending-ack transitions are enforced by API shape, not only by tests
