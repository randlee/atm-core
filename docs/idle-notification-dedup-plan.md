# Idle Notification Dedup Plan

## 1. Purpose

This document compares the current ATM-core documentation against the requested
`idle_notification` behavior and defines the planning gap that must be closed
before implementation.

Requested behavior from `team-lead`:
- retain at most one `idle_notification` per sender in any inbox
- when a new `idle_notification` arrives from the same sender, replace/remove
  the previous unread idle notification from that sender instead of appending a
  second retained copy

Current PG.1 sprint scope:
- define sender-scoped dedup in product requirements
- record the resolved idle-notification detection rule
- defer read-time auto-purge and daemon-side removal behavior

This is a planning-only document. It does not authorize implementation on its
own.

## 2. Current Documented State

### 2.1 Product Requirements

Current product requirements mention idle notifications only in the clear
command:
- `docs/requirements.md:639` lists `--idle-only` as a supported `atm clear`
  flag
- `docs/requirements.md:660` says `--idle-only` narrows removal to
  idle-notification messages only

There is no current requirement text for:
- send-time deduplication or replacement of idle notifications
- mailbox-level idle-notification uniqueness by sender
- read-time auto-purge of idle notifications
- a dedicated idle-notification schema marker beyond the implicit concept used
  by `clear --idle-only`

### 2.2 Product Architecture

Current product architecture mentions idle notifications only in the clear
pipeline:
- `docs/architecture.md:439` includes `idle-only flag` in `ClearQuery`
- `docs/architecture.md:560` says the clear pipeline applies optional age and
  idle-only filters

There is no architectural ownership yet for:
- detecting that a just-arrived message is an idle notification from sender X
- replacing/removing the older unread idle notification from sender X in the
  target inbox during send/append
- purging a displayed idle notification during the read writeback phase

### 2.3 Cross-Cutting Read Behavior Doc

`docs/read-behavior.md` does not mention idle notifications today.

It also still carries one stale statement that conflicts with current product
requirements:
- the `Legal Workflow Transitions` section used to say clear may remove
  pending-ack with an explicit override
- `docs/requirements.md:655-664` no longer allows any pending-ack clear
  override

That stale statement was corrected in PG.1 so the idle-notification plan is no
longer layered on top of contradictory clear semantics.

## 3. Gap Summary

The current spec supports only manual cleanup of idle notifications through
`atm clear --idle-only`.

The broader requested behavior needs two new capabilities that are not
documented yet:

1. sender-scoped dedup/replacement on arrival
2. read-time auto-purge instead of preserving idle notifications in history

PG.1 closes only the first item. Read-time auto-purge remains a follow-on.

That means the existing docs are inadequate in four places:
- requirements: no functional requirement for idle-notification replacement and
  the auto-purge behavior remains deferred
- architecture: no service ownership for idle-notification lifecycle rules
- read behavior: no classification/writeback rule for the deferred
  auto-purge-on-read follow-on
- schema/workflow notes: no explicit way to identify an idle notification as a
  first-class mailbox concept

## 4. Where Implementation Would Need To Change

This section identifies the likely write scopes once implementation is
approved.

### 4.1 Send Path / Mailbox Append Boundary

Likely ownership:
- `atm-core` mailbox append logic
- send service request/envelope construction only if sender-side tagging is
  incomplete today

Needed behavior:
- recognize idle notifications at append time
- before appending a new idle notification to a target inbox, remove any older
  unread idle notification from the same sender in that same inbox
- append the new idle notification as the surviving record

Why this belongs here:
- uniqueness by sender is a mailbox persistence rule, not a CLI formatting rule
- the replacement must be atomic with the new append to avoid races that retain
  duplicates

### 4.2 Read Path / Read Writeback Boundary

Likely ownership:
- `atm-core` read service and mailbox writeback logic

Needed behavior:
- when a displayed message is an idle notification and the read operation is
  performing normal mark-as-read mutation, remove the message from the owning
  inbox file instead of preserving it as `(Read, NoAckRequired)` or history
- apply this removal during the same atomic writeback phase that would
  otherwise persist `read = true`

Why this belongs here:
- the requested behavior is specifically tied to `atm read`
- the purge must follow display/selection rules rather than happen as a later
  independent cleanup task

Status:
- deferred after PG.1

### 4.3 Clear Path

Likely ownership:
- `atm-core` clear service

Needed behavior:
- current `clear --idle-only` may still remain useful for backlogged inboxes or
  for manual cleanup of legacy retained duplicates
- however, it should no longer be the primary mechanism for keeping idle
  notifications under control once send/read behavior is changed

Planning implication:
- `clear --idle-only` becomes a compatibility/backlog cleanup surface, not the
  main lifecycle policy

### 4.4 Schema / Message Classification

Likely ownership:
- product requirements
- `atm-core` mailbox/workflow classification docs

Needed behavior:
- define how an inbox record is identified as an idle notification
- make that detection explicit enough that send/read/clear can agree on it

Open design options:
- explicit `messageKind = "idle_notification"`
- explicit boolean/enum field attached to the envelope
- derived classification from text/summary alone (not recommended)

Recommendation:
- use an explicit envelope field or message-kind enum value
- do not rely on free-text matching for lifecycle behavior

## 5. Proposed Requirement Additions

### 5.1 New Product Requirement

Proposed new requirement ID:
- `REQ-P-IDLE-001`

Draft language:
- ATM shall treat idle notifications as a special non-actionable notification
  class.
- For any target inbox, ATM shall retain at most one unread idle notification
  per sender.
- When a new idle notification from sender `S` is delivered to inbox `I`, ATM
  shall atomically remove any older unread idle notification from sender `S` in
  inbox `I` before appending the new record.
- Idle-notification lifecycle rules shall not apply to non-idle message kinds.

Deferred from PG.1:
- read-time auto-purge on `atm read`
- daemon-side idle-notification removal behavior

### 5.2 Send Requirement Changes

`REQ-P-SEND-001` should be extended to state:
- send/mailbox-append logic applies idle-notification deduplication rules when
  the outgoing envelope is classified as an idle notification
- the dedup/removal plus append occurs atomically in the mailbox append
  boundary

### 5.3 Read Requirement Changes

`REQ-P-READ-001` should be extended to state:
- idle notifications are non-actionable and do not belong in the pending-ack
  queues
- read-time auto-purge remains deferred after PG.1
- `--no-mark` leaves the message untouched and therefore does not auto-purge it
  when that behavior is implemented later

### 5.4 Clear Requirement Changes

`REQ-P-CLEAR-001` should be clarified to state:
- `--idle-only` remains available for manual backlog cleanup and legacy inbox
  repair
- idle-notification deduplication and read-time auto-purge are the primary
  lifecycle controls, so `clear --idle-only` is secondary

## 6. Proposed Architecture Additions

`docs/architecture.md` should gain:
- an explicit idle-notification lifecycle subsection under mailbox/read
  architecture
- send-path ownership note that mailbox append performs sender-scoped idle
  dedup atomically
- read-pipeline ownership note that read writeback may delete a displayed idle
  notification instead of persisting a read-state mutation
- clear-pipeline note that idle-only clear is manual cleanup, not the primary
  lifecycle path

`docs/atm-core/modules/mailbox.md` should eventually own the atomic dedup and
replacement rule.

`docs/atm-core/modules/read.md` should eventually own the read-time purge rule.

## 7. Edge Cases And Questions

### 7.1 Replacement Scope

Question:
- should a new idle notification replace only older unread idle notifications,
  or any prior idle notification from the same sender?

Recommended answer:
- replace only older unread idle notifications at append time
- once an idle notification has already been read, normal read-time auto-purge
  should have removed it anyway

### 7.2 `--no-mark`

Question:
- if a user reads with `--no-mark`, should the idle notification still be
  auto-purged?

Resolved ruling:
- no
- `--no-mark` should preserve current "display without mutation" semantics,
  which implies no auto-purge when that behavior is implemented later
- this keeps the deferred purge rule aligned with writeback rather than display
  alone

### 7.3 Reading Another Agent's Inbox

Question:
- should reading another agent's inbox purge their idle notifications?

Recommended answer:
- no by default
- purge should apply only when the command would otherwise perform writeback in
  the owning inbox under normal marking semantics for that actor
- this avoids cross-actor destructive cleanup during inspection

### 7.4 Duplicate Legacy Backlog

Question:
- what happens to inboxes that already contain many duplicate idle
  notifications from the same sender?

Recommended answer:
- keep `clear --idle-only` as a manual repair path
- once implementation lands, newly arriving idle notifications stop adding to
  the duplicate backlog
- optional later enhancement: `atm clear --idle-only` may dedupe to newest per
  sender instead of deleting all idle notifications, if that is operationally
  preferred

### 7.5 Idle Notification Identification [RESOLVED]

Question:
- what exact field identifies an idle notification today?

Resolved detection rule:
- ATM detects an idle notification by parsing the persisted message `text`
  field as JSON and checking for `type == "idle_notification"`
- if parsing fails, or the parsed object has a different `type`, ATM treats the
  message as a normal message
- the idle-notification marker is therefore observable from existing mailbox
  data and does not require a new schema field

## 8. Recommended Next Doc Changes Before Implementation

1. Add `REQ-P-IDLE-001` to `docs/requirements.md`.
2. Update `REQ-P-SEND-001` with the sender-scoped idle-notification
   deduplication rule.
3. Record the resolved text-field JSON detection rule in the product docs.
4. Keep read-time auto-purge deferred until a later sprint defines the exact
   read/writeback semantics.
5. Update `docs/architecture.md` to assign send/read/mailbox ownership for
   dedup and the deferred auto-purge follow-on.
6. Update `docs/read-behavior.md` to explain how idle notifications interact
   with display, marking, `--no-mark`, and history when the deferred read-time
   purge work is scheduled.
7. Correct the stale pending-ack clear-override text in `docs/read-behavior.md`
   so cleanup semantics remain internally consistent.
8. After the doc update, create the follow-on implementation sprint that owns:
   - mailbox append dedup
   - read writeback auto-purge
   - compatibility cleanup behavior for legacy duplicate idle notifications
