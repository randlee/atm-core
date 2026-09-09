# Phase AZ issue inventory

Planning identifiers in this file track scope and sprint ownership. They do not
replace repository issue numbers or QA triage authority.

| Planning id | Status | Owning sprint | Required closure evidence |
| --- | --- | --- | --- |
| `AZ-LONG-NUDGE` | planned | AZ.1 | All Steer, Queue/rebuild, Task/reminder, graft, and Herdr projections use persisted title metadata; ordinary/J2 body sentinels are absent. |
| `AZ-TASK-LIFECYCLE` | planned | AZ.2 | Corrected state table, stable logical id, immutable attempts/events, priority, migration, database invariants, idempotency, and concurrency tests pass. |
| `AZ-TASK-COMPLETE-PENDING` | planned | AZ.2 | Block/close/reassign/reopen/supersede atomically invalidate pending nudges for every ineligible assignment attempt, including legacy rows. |
| `AZ-TASK-COMMANDS` | planned | AZ.3 | Canonical task CLI/API, authorization, explicit start, durable handoffs, supersession, and legacy adapters pass end-to-end. |
| `AZ-IDLE-INTERLEAVING` | planned | AZ.4 | One derived selector emits at most one item per idle opportunity and alternates the separate ephemeral/persistent lanes across restart. |

## Binding clarifications

- The earlier shorthand `Active <-> Blocked` is superseded. Legal blocking
  transitions are `Assigned -> Blocked`, `Active -> Blocked`, and explicit
  `Blocked -> Assigned` with a resolution note. Unblock preserves priority and
  original assignment time and never starts the task.
- An idle opportunity derives one `AttentionItem`: either an ephemeral queued
  message or a persistent task reminder. Their lifecycle storage remains
  separate; only a small scheduler cursor records which lane is next.
- A material objective change is never an in-place edit. It closes the old task
  as `Aborted(Superseded)` and creates a linked, distinct successor `TaskId`.

## Exclusions

- No Phase AZ sprint modifies the frozen legacy synchronous daemon.
- ATM does not own Beads task details; a Beads id may only be used as the opaque
  `TaskId`.
- No live daemon/test-daemon, tag, release, publish, or installation action is a
  Phase AZ validation step.
