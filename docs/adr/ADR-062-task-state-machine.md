---
title: Task State Machine
---

# ADR-062 — Task State Machine

Date: 2026-09-04

## Decision

ATM task state is a daemon-owned, message-derived ledger. `TaskStore` is the
seventh sealed optional storage capability (as re-counted by ADR-054), and is
backend-neutral: it exposes task/event reads plus reminder and lead audit
appends, not state mutation. SQLite owns the `tasks` and append-only
`task_events` tables.

The pure state machine has states `Assigned`, `Active`, and `Complete`; events
are `Assigned`, `Acked`, and `Completed`. The transition table is:

| Current | Assigned | Acked | Completed |
| --- | --- | --- | --- |
| ∅ | Assigned | no-op | reject |
| Assigned | Assigned | Active | Complete |
| Active | Active | Active | Complete |
| Complete | reject | reject | reject |

Only local message admission applies assignment/completion, and only the
acknowledgement writer operation applies acknowledgement. Peer-originated
receipts are stored with `MessageWriteOrigin::Peer` and never transition the
ledger. A local resend refreshes the assignment message id and description but
does not change state. Completion may be authored by the assignee or assigner;
it rejects a missing or completed task. Acknowledging an assigned task rejects
when another task is active for that assignee. Rejections use a recovery hint
to inspect task events and roll back the enclosing writer transaction.

The audit replay claim is per `(team, task_id, assignee)`: replaying accepted
`Assigned`/`Acked`/`Completed` events through this table reproduces state; the
latest assignment event supplies `assignment_message_id`; reminder and lead
event counts reproduce their counters. Description is intentionally not
replayable.

## Reminder cycle

AZ.4 replaces the historical drain-first reminder sequence with one fair idle
attention selector. Each accepted canonical `Idle` roster revision has one
durable opportunity which may reserve zero or one identifier-only item:
ephemeral queued message or persistent task reminder. With both lanes due, the
persisted cursor alternates lanes; task priority orders only within the task
lane. The selector revalidates the exact idle revision and selected queue or
task/attempt immediately before emitting the bounded nudge metadata. It never
re-sends a task body.

A task reminder is eligible only for the current `Assigned` or `Active`
attempt and only after its 60-second current-attempt cadence. `Blocked` and
`Closed` tasks receive no normal reminder; unblock returns work to assigned
ordering without starting it. Successful delivery appends a `RecordReminder`
audit and advances the task-scoped ordinal, without acknowledging or changing
task state. The tenth, twentieth, and later tenth successful reminders retain
the lead-escalation boundary. Queue claim/release/requeue remains owned by
`PendingNudgeStore`; transient scheduler failure keeps the same durable
reservation until its fifth failure terminalizes the reservation, not the
underlying message or task.

## Lead notification and escalation

After a successful reminder is recorded, every tenth reminder is an escalation
boundary: reminders 10, 20, 30, and so on notify the one roster member whose
`agent_type` is `lead`. The lead mail is deferred daemon-originated mail and is
audited as `LeadNotified` only when the lead write succeeds. A missing or
ambiguous lead suppresses that audit event without suppressing configured
escalation-recipient fan-out.

Blocked runtime observations create an in-memory episode. The first escalation
is eligible after 60 seconds and subsequent notifications are eligible every
10 minutes. An episode is stamped only when at least one lead or configured
recipient write, or the Herdr notification, succeeds; total failure therefore
retries on the next eligible tick. Lead and recipient failures are independent
and are surfaced in queue-pump statistics.

Escalation recipients are stored by `TaskStore` in daemon scope or an explicit
team scope. A team list replaces the daemon list for that team; absent team
configuration falls back to the daemon list. The CLI accepts ADR-040 recipient
forms through `atm escalation add|remove|list [--team <name>]`, validates them
before persistence, and the doctor reports both effective recipients and
their source. Fan-out is capped at eight recipients per escalation.

## Consequences

This is a fresh Phase AX design, not restoration of the AC.6 scaffolding. It
replaces the historical Claude-code/Pydantic deferral because ATM tasks are
cross-host records derived from messages the Rust daemon already persists.

## Amendment — Phase AZ.2 logical lifecycle

ADR-063 governs the replacement of the message-derived three-state model with
one stable logical `(team, TaskId)` record. `Assigned`, `Active`, and `Blocked`
are open; `Closed(TaskOutcome)` is terminal. Attempts and events are immutable,
and `TaskOperationId` is independent of message identity. The pure transition
table makes `Blocked -> Assigned` explicit and prohibits ordinary success from
an unstarted assignment; only the retained legacy-completion adapter has that
compatibility provenance. Reassign/reopen add attempts under the same identity;
supersede closes the old task as aborted and creates a distinct successor.

SQLite v2 is authoritative. `tasks`/`task_events` and their description field
remain a version-bounded 1.6.x bridge, not a second lifecycle authority. The
sealed async mutation boundary owns idempotency, CAS, message/handoff
persistence, events, and task-id-joined pending-marker cleanup in one writer
transaction.
