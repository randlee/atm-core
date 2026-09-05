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

## Consequences

This is a fresh Phase AX design, not restoration of the AC.6 scaffolding. It
replaces the historical Claude-code/Pydantic deferral because ATM tasks are
cross-host records derived from messages the Rust daemon already persists.
