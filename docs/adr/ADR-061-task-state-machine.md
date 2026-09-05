---
title: Task State Machine
---

# ADR-061 — Task State Machine

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

The Tokio-owned Herdr queue wake pump polls every 5 seconds. After it drains
ordinary deferred mail, it checks open tasks for Herdr-backed members reported
as idle, done, or blocked. It selects the oldest active task (or the oldest
assigned task when none is active) and re-sends the Task body at most once per
member every 60 seconds. Drain comes first and shares the same per-tick prompt
budget, so a queue prompt counts as that member's reminder attempt for the
tick.

This is Herdr-only. A member that has moved to another backend receives no
reminder from this pump. `idle_members` retains its queue-dashboard meaning:
it counts only members that are both idle and have pending deferred mail.

Blocked members are recorded as runtime state `blocked`, receive no Herdr
prompt, and append a `reminded` audit event with outcome `blocked` on the same
cadence. Rendering failures append `unrenderable`; successful emissions append
`emitted`. These events update reminder bookkeeping only and never transition
task state. If the optional task store is unavailable, only the reminder step
is skipped; deferred-mail draining continues.

## Consequences

This is a fresh Phase AX design, not restoration of the AC.6 scaffolding. It
replaces the historical Claude-code/Pydantic deferral because ATM tasks are
cross-host records derived from messages the Rust daemon already persists.
