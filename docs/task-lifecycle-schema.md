# Logical task lifecycle schema

Phase AZ.2 stores one canonical logical task per `(team, task_id)` in
`tasks_v2`. The retained `tasks` and `task_events` tables are a 1.6.x
compatibility projection only; their `description` column is never copied into
canonical rows.

The canonical projection contains the current assignee, lifecycle state,
priority, original assignment time, current immutable assignment-attempt
number, reminder ordinal, revision, and update time. Assignment attempts refer
to a canonical message id and optional template SHA. Lifecycle events refer to
an operation id and optional related task id. Neither relation stores rendered
message text, template bytes, template variables, task description, or Beads
details.

## Lifecycle and ordering

`Assigned`, `Active`, and `Blocked` are open states. `Closed` always carries a
`Succeeded`, `Failed`, or `Aborted(Cancelled|Superseded { successor_task_id })`
outcome. `Blocked -> Assigned` is the only unblock transition; it never starts
work. An unstarted assignment may fail or abort, but only the retained legacy
completion adapter may close it successfully. Reassign and reopen create a new
numbered immutable attempt without changing logical task identity; supersede
closes the old task as aborted and creates a distinct successor atomically.

Open task lists order Active first, then Assigned by High/Normal/Low priority,
original assignment time, and task id, then Blocked. Closed history orders by
terminal update time descending and task id. `top_runnable_task` returns the
Active task or the first Assigned task; it never returns Blocked or Closed.

## Coexistence and mutation

`STORAGE_SCHEMA_VERSION = 2.1.0` contains ADR-061's approved 2.0.0 major task
domain migration plus AZ.4's additive attention tables. ATM 1.6.x retains a
bidirectional v1 bridge; 1.7.0 is the earliest separately approved removal
target. Migration collapses v1 rows by `Active > Assigned > Complete`, records
each source row, and deterministically demotes surplus active rows before the
`(team, current_assignee)` active unique index exists. The additive
`attention_lane_cursors` and `attention_opportunities` tables are installed by
an idempotent 2.1 ensure step, so a fresh 2.1 database and an upgraded 2.0
database converge without changing v1 compatibility rows.

`AsyncTaskMutationStore` is the sole v2 mutation capability. Every request has
an independent `TaskOperationId` and optional expected revision. The SQLite
writer transaction persists any prepared assignment or handoff message, task
projection, attempt/event records, operation replay result, and task-id-joined
pending-marker cleanup together. Replaying identical input returns the original
result; reusing an operation id with different input or a stale revision fails
closed.

Prepared assignments carry their already-admitted delivery origin. Only a
locally admitted recipient is eligible for this transaction; a peer-origin
recipient is rejected before any message, operation, task, attempt, or event
is written with `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED`. Cross-host task
handoff remains outside this storage transaction boundary.

## Idle attention scheduling

The scheduler is not another task or message lifecycle table. It owns only a
per-member next-lane cursor and an `IdleOpportunityId` reservation containing
the selected lane plus message or task/attempt identities. It never stores a
body, rendered template text, or task description. The queue remains the owner
of queue claim/release/requeue; the task ledger remains the owner of task state
and attempt-aware reminder audit.

For one canonical `Idle` roster revision, the selector reserves zero or one
item. When both lanes are due it alternates ephemeral message then persistent
task across committed reservations; task priority orders only candidates within
the persistent lane. A selected task is revalidated against its current
assignee, attempt, state, and 60-second cadence immediately before emission.
Blocked and closed tasks are ineligible. Successful task reminders append a
`RecordReminder` event for the current attempt; they do not acknowledge, start,
or close work. Reassignment invalidates the old attempt and unblock returns the
task to assigned ordering without activation.

Delivery retries keep the same reservation and item identity. Each retryable
failure increments durable `failed_attempts`; the fifth is
`PermanentlyFailed`. That terminalizes the reservation only, never its message
or task. A successful ephemeral nudge consumes its queue claim; read or
acknowledged messages that lose the conditional claim are suppressed.
