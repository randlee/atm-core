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

`STORAGE_SCHEMA_VERSION = 2.0.0` is an ADR-061 approved major change. ATM
1.6.x retains a bidirectional v1 bridge; 1.7.0 is the earliest separately
approved removal target. Migration collapses v1 rows by `Active > Assigned >
Complete`, records each source row, and deterministically demotes surplus
active rows before the `(team, current_assignee)` active unique index exists.

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
