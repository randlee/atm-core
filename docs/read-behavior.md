# ATM Read Behavior Review

This document defines the canonical read behavior for the rewrite and records which parts of current ATM behavior are preserved.

## 1. Why This Document Exists

The current read path mixes several concerns:
- mail visibility
- workflow axes
- display buckets
- watermark tracking
- wait-mode behavior

That makes the command hard to reason about and easy to regress.

The rewrite keeps the useful read behavior but makes the model explicit:
- canonical message axes
- display bucket mapping
- selection policy
- legal state transitions

## 2. Best Current ATM Behavior To Preserve

The current command already has useful queue behavior that should survive the rewrite:
- default view shows actionable work only
- pending-ack messages stay visible until they are acknowledged
- task-linked ack-required messages arrive already actionable
- duplicate deliveries should collapse by `message_id` instead of showing the
  same message repeatedly
- history can be expanded without hiding actionable work
- `--all` shows everything
- older unread messages remain visible even when the seen-state watermark is newer
- last-seen updates from the latest displayed message, not the latest message in the inbox

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

Current behavior when reading a message should become:
- displayed messages are always written back with `read = true`
- displayed unread messages in your own inbox also receive `pendingAckAt` when marking is enabled and the message did not already require acknowledgement
- displayed unread messages that already require acknowledgement remain pending-ack after display

Current ack behavior:
- an acknowledged message receives `acknowledgedAt`
- `pendingAckAt` is removed
- a reply message is emitted and references the original message id

Current clear behavior that must survive:
- clear removes acknowledged messages
- pending-ack messages are not clearable by default
- an explicit override may clear stale pending-ack entries

This behavior is messy in the current code because the state machine is implicit. The rewrite keeps the behavior but makes the state model explicit.

## 4. Canonical Two-Axis Model

The retained workflow has two canonical axes.

Read axis:
- `Unread`
- `Read`

Ack axis:
- `NoAckRequired`
- `PendingAck`
- `Acknowledged`

Classification rule:
- read axis:
  - `read = false` => `Unread`
  - `read = true` => `Read`
- ack axis:
  - `acknowledgedAt` present => `Acknowledged`
  - else `pendingAckAt` present => `PendingAck`
  - else => `NoAckRequired`

Derived message class:
- `PendingAck` when the ack axis is pending
- `Acknowledged` when the ack axis is acknowledged
- `Unread` when the read axis is unread and the ack axis is not pending or acknowledged
- `Read` otherwise

Important distinction:
- canonical read axis plus ack axis is the domain model
- display buckets are the CLI presentation model

## 5. Display Bucket Mapping

Display bucket mapping from the derived message class:
- `Unread` => `unread`
- `PendingAck` => `pending_ack`
- `Acknowledged` => `history`
- `Read` => `history`

That means:
- history is a bucket, not a state
- `Acknowledged` and `Read` must remain distinct in the model even though both render into history

## 6. Legal Workflow Transitions

```text
Send normal message
  -> (Unread, NoAckRequired)

Send ack-required message
  -> (Unread, PendingAck)

Send task-linked message
  -> persist taskId
  -> (Unread, PendingAck)

Read own inbox, marking enabled
  (Unread, NoAckRequired) -> (Read, PendingAck)
  (Unread, PendingAck) -> (Read, PendingAck)

Read own inbox, --no-mark
  (Unread, NoAckRequired) -> (Read, NoAckRequired)
  (Unread, PendingAck) -> (Read, PendingAck)

Read other inbox
  (Unread, NoAckRequired) -> (Read, NoAckRequired)
  (Unread, PendingAck) -> (Read, PendingAck)
  (Read, PendingAck) -> (Read, PendingAck)
  (Read, Acknowledged) -> (Read, Acknowledged)
  (Read, NoAckRequired) -> (Read, NoAckRequired)

Ack workflow
  (Read, PendingAck) -> (Read, Acknowledged)
  and emit a reply message referencing the original message id

Clear workflow
  remove only (Read, NoAckRequired) and (Read, Acknowledged)
  with explicit pending-ack override, also allow removal of pending-ack entries
```

Disallowed transitions:
- any transition that makes the read axis move from `Read` back to `Unread`
- `Acknowledged -> PendingAck`
- `Acknowledged -> NoAckRequired`
- clearing a pending-ack message without the explicit override
- clearing an unread message
- any transition that skips the legal graph

Notes:
- `read = true` is the base mutation on display
- task-linked messages are required-ack messages and remain in the pending-ack queue until acknowledged

## 7. Seen-State Rules

Seen-state is a selection policy, not a state transition.

Rules:
- enable it by default
- `--since-last-seen` explicitly re-enables the default watermark filter
- disable it with `--no-since-last-seen`
- bypass it with `--all`
- keep older unread messages visible
- keep older pending-ack messages visible
- allow the watermark to hide only history items

If both `--since-last-seen` and `--no-since-last-seen` appear, `--no-since-last-seen` wins.

Watermark update rule:
- update from the latest displayed message only
- do not include filtered-out messages

`--no-update-seen` leaves the watermark unchanged after the read, even when messages were displayed.

`--since <iso8601>` filters by message timestamp greater than or equal to the given value.

`--from <name>` filters by sender name.

Duplicate-collapse rule:
- if multiple entries share the same non-null `message_id`, show only the most
  recent entry
- suppress earlier duplicates silently
- if a record lacks `message_id`, do not merge it with any other record

## 8. Wait-Mode Rules

`--timeout` preserves the current queue-first behavior:
- if the requested selection already contains unread or pending-ack messages at command start, return immediately
- block only when the requested selection is empty at command start
- while blocked, wake only when a newly arrived message becomes eligible for the requested selection
- preserve the same sender, timestamp, seen-state, and selection filters during the wait
- use native file watching with a 100ms safety poll while the watcher is active
- fall back to 2-second polling if the native watcher cannot be initialized
- apply read-triggered marking only after the wait completes and the final displayed selection is chosen

## 9. Required API Shape

The core read pipeline must encode the transition rules in types.

Minimum shape:

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

pub enum DisplayBucket {
    Unread,
    PendingAck,
    History,
}

pub enum AckActivationMode {
    PromoteDisplayedUnread,
    ReadOnly,
}

pub struct StoredMessage<R, A> {
    // persisted fields
    // state markers
}

pub struct UnreadReadState;
pub struct ReadReadState;
pub struct NoAckState;
pub struct PendingAckState;
pub struct AcknowledgedAckState;

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

Classification boundary:
- wire schema -> read axis + ack axis + derived class

Rendering boundary:
- stateful core model -> CLI display buckets and rows

## 10. Recommended Read Algorithm

1. Resolve actor identity and target inbox.
2. Build the hostname registry for configured origin inboxes.
3. Load the merged inbox surface.
4. Convert wire records into canonical axis-typed messages and derive the display class.
5. Apply sender and timestamp filters (`--from`, `--since`).
6. Apply seen-state filtering unless selection is `All`.
7. Map derived message classes to display buckets and apply selection mode.
8. If `--timeout` is set and the current selection is empty, wait for a newly eligible message.
9. Sort newest-first and apply limit.
10. Apply legal read-axis and ack-axis transitions for displayed messages if allowed.
11. Persist state changes atomically.
12. Update seen-state from the displayed set when enabled.
13. Return `ReadOutcome`.

This order matters.

In particular:
- selection must happen before mutation
- mutation must happen before final output is returned
- seen-state updates must use the displayed set, not the full inbox
- when the merged inbox surface includes origin inbox files, each displayed-message mutation must be written back to the physical source file for that record

## 11. Output Contract

Human output:
- queue heading
- bucket counts line
- unread bucket
- pending-ack bucket
- optional history bucket
- hidden-history line when history is collapsed

JSON output:
- `action = "read"`
- `team`
- `agent`
- `messages` (selected messages only)
- `count`
- `bucket_counts`
- `history_collapsed`

Cross-document invariants:
- displayed messages always persist `read = true`
- task-linked messages are ack-required from send time
- pending-ack messages remain actionable until acknowledged
- `atm clear` never removes unread messages
- `atm clear` removes pending-ack messages only when the explicit override is
  set
- `--timeout` returns immediately when the requested selection is already non-empty

`bucket_counts` fields:
- `unread`
- `pending_ack`
- `history`

## 12. Review Standard

An implementation of `atm read` is acceptable only if:
- it uses the canonical two-axis workflow model
- it keeps display buckets separate from the canonical axes
- it preserves default actionable-queue behavior
- it preserves the current pending-ack lifecycle
- it preserves task-linked pending-ack visibility until acknowledgement unless
  the operator explicitly invokes the pending-ack clear override
- no daemon-only logic survives in core read behavior
- read-axis and ack-axis transitions are enforced by API shape, not only by tests
