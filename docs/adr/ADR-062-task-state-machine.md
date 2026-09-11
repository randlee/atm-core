---
title: Task State Machine
---

# ADR-062 — Task State Machine

Date: 2026-09-04
Amended: 2026-09-11 (Phase BA)

## Phase AX decision (as built; superseded where marked)

Sentences marked *Superseded (BA)* are replaced by the Phase BA amendment
below, which is the normative task contract. Unmarked sentences remain active.

ATM task state is a daemon-owned, message-derived ledger. `TaskStore` is the
seventh sealed optional storage capability (as re-counted by ADR-054), and is
backend-neutral: it exposes task/event reads plus reminder and lead audit
appends, not state mutation. SQLite owns the `tasks` and append-only
`task_events` tables.

*Superseded (BA):* The pure state machine has states `Assigned`, `Active`, and
`Complete`; events are `Assigned`, `Acked`, and `Completed`. The as-built
transition table is:

| Current | Assigned | Acked | Completed |
| --- | --- | --- | --- |
| ∅ | Assigned | no-op | reject |
| Assigned | Assigned | Active | Complete |
| Active | Active | Active | Complete |
| Complete | reject | reject | reject |

*Superseded (BA):* Only local message admission applies
assignment/completion, and only the acknowledgement writer operation applies
acknowledgement. Peer-originated receipts are stored with
`MessageWriteOrigin::Peer` and never transition the ledger. A local resend
refreshes the assignment message id and description but does not change state.
Completion may be authored by the assignee or assigner; it rejects a missing
task. *Superseded (BA):* it rejects a completed task. *Superseded (BA):*
Acknowledging an assigned task rejects when another task is active for that
assignee. Rejections use a recovery hint to inspect task events and roll back
the enclosing writer transaction.

*Superseded (BA):* The audit replay claim is per `(team, task_id, assignee)`:
replaying accepted `Assigned`/`Acked`/`Completed` events through this table
reproduces state. The latest assignment event supplies `assignment_message_id`;
reminder and lead event counts reproduce their counters. Description is
intentionally not replayable.

## Reminder cycle

*Superseded (BA):* The Tokio-owned Herdr queue wake pump polls every 5 seconds. After it drains
ordinary deferred mail, it checks open tasks for Herdr-backed members reported
as idle, done, or blocked. *Superseded (BA):* It selects the oldest active
task (or the oldest assigned task when none is active). It re-sends the Task
body at most once per member every 60 seconds. Drain comes first and shares the same per-tick prompt
budget, so a queue prompt counts as that member's reminder attempt for the
tick.

Superseded by Phase BA (docs/plans/phase-ba/phase-ba-plan.md §4, sprint BA.3): the reminder cycle runs regardless of delivery backend; a task reminder is emitted only when the assignee is Idle with no pending mail, at most once per `TASK_REMINDER_INTERVAL_MS`; `Blocked` outcomes are recorded for offline members and escalate to the lead after `TASK_STALLED_REMINDER_THRESHOLD`.

## Lead notification and escalation

*Superseded (BA):* After a successful reminder is recorded, every tenth
reminder is an escalation boundary: reminders 10, 20, 30, and so on notify the
one roster member whose `agent_type` is `lead`. The lead mail is deferred daemon-originated mail and is
audited as `LeadNotified` only when the lead write succeeds. A missing or
ambiguous lead suppresses that audit event without suppressing configured
escalation-recipient fan-out.

*Superseded (BA):* Blocked runtime observations create an in-memory episode. The first escalation
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

## Phase BA amendment (2026-09-11) — normative

Source: `docs/plans/phase-ba/nudge-task-design.md`. Supersedes the marked
sentences above. Acknowledgement is message hygiene only: it is not a task
event, is never gated on task state, and never transitions a task.

### Identity and constraints

| Rule | Form |
| --- | --- |
| One row per task | `PRIMARY KEY (team, task_id)`; `assignee` is a column, not identity |
| At most one active task per agent | `CREATE UNIQUE INDEX one_active_task_per_agent ON tasks(team, assignee) WHERE state = 'active'`; any number of `assigned` rows per agent |
| Queue order | `ORDER BY position, assigned_at, task_id`; `position` is a separate column, default end of queue; `assigned_at` records the current assignment and is reset by reassign/reopen, never by move |
| Close outcome | typed `completed \| refused \| cancelled`; free text is a human-facing reason only |
| Reassignment | `assign` on an existing open id updates assignee/state/placement in place and appends `reassigned`; a closed id is reopened in place with `reopened` |
| Replay | per `(team, task_id)`, fold events in ascending `seq`; `assigned` establishes initial state, `started`, outcome events, `reassigned`, `reopened`, and `migrated` change state, while `moved`, `rejected`, `reminded`, `lead_notified`, and `acked` are state-neutral |

### States and events

States: `assigned`, `active`, `complete`. Events: `Assigned`, `Started`,
`Reassigned`, `Reopened`, `Completed(outcome)`.

| Current | Assigned | Started | Completed(outcome) |
| --- | --- | --- | --- |
| ∅ | assigned | reject | reject |
| assigned | assigned (resend) | active; reject when another task is active for the assignee | complete |
| active | active (resend) | active | complete |
| complete | assign reopens the same id | reject | no transition; deliver and inform already complete |

`Started` notifies the assigner. `Completed(outcome)` dequeues the task (no
further reminders) and appends one timestamped event carrying the outcome;
these are two facts recorded together. A close that fails never discards the
carried message.

### Reminder and escalation

| Condition | Action |
| --- | --- |
| assignee `Idle` with an incomplete task | remind, at most once per `TASK_REMINDER_INTERVAL_MS` (60 s) per member; queued messages are discharged first (ADR-054, Phase BA amendment) |
| assignee `Active` | none; never divert |
| assignee `Blocked` or `Offline` | no reminder; one escalation message per episode (first accepted observation of the state until the first accepted observation of another state), no cooldown, no re-notification |
| `reminder_count` reaches `TASK_STALLED_REMINDER_THRESHOLD` (10) | escalate once; reminders stop until the task changes state (start or close) or is reassigned or reopened; a change in the assignee's runtime state alone does not resume nudging. |

Task selection for an idle member: the active task, else the first `assigned`
task in queue order.

## Consequences

The lifecycle remains one row per task id, with explicit assignment events for reassignment and reopen; close outcomes remain limited to completed, refused, and cancelled.

