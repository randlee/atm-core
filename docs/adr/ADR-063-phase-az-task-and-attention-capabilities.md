# ADR-063 — Phase AZ Task And Attention Storage Capabilities

| Field | Value |
| --- | --- |
| ID | ADR-063 |
| Status | Proposed — blocked on ADR-061 major-change approval |
| Scope | Phase AZ task lifecycle mutation, attention scheduling, and SQLite v2 coexistence |
| Relates to | ADR-009, ADR-018, ADR-035, ADR-036, ADR-054, ADR-061, ADR-062, Phase AZ |

## Context

The existing task ledger derives lifecycle from per-assignee message rows. It
cannot represent stable logical identity across assignment attempts, explicit
blocked and terminal outcomes, atomic supersession, or idempotent multi-row
mutation. Phase AZ also replaces Herdr's drain-first reminder sequence with a
durable, fair selector over separate message and task lanes.

Those changes require three new sealed storage capability traits and a major
SQLite task-schema migration. ADR-018 requires a follow-up ADR after four
optional capabilities; ADR-054 recounts six, then records `TaskStore` as the
seventh. ADR-061 separately requires explicit approval and coexistence for a
major storage interface change.

## Decision

### D1. Stable task domain and mutation owner

`atm-storage` owns the backend-neutral task state, immutable assignment
attempts, append-only events, and task operation/result types. Open state and a
terminal outcome are unrepresentable together in Rust. SQLite may project the
sum type into checked columns for indexed reads.

`AsyncTaskMutationStore` is the eighth optional capability. It owns one
bounded asynchronous mutation admission that atomically applies assignment,
start, block/unblock, reassign/reopen, terminal handoff, and supersession. It is
separate from `TaskStore` because it is a compare-and-swap transactional command
boundary, not CRUD growth or a caller-owned sequence of writes. It accepts no
SQLite handle, renderer, HTTP DTO, or message body.

`TaskOperationId` is an independent opaque ULID newtype and idempotency-key
namespace, never an alias for `AtmMessageId`. A dedicated task-operations table
owns one request fingerprint and typed result per `(team, operation_id)`;
multiple events across old and successor tasks may reference one operation.

### D2. Attention schedule capabilities

`AttentionScheduleStore` is the ninth optional capability. It owns only durable
per-member lane cursors and one-item idle-opportunity reservations. It does not
own message lifecycle, task lifecycle, bodies, templates, emitters, or runtime
presence.

`AsyncAttentionScheduleStore` is the tenth optional capability. Although it is
the Tokio-safe companion of the same semantic store, this ADR counts it
independently rather than understating the inventory. It exists because the
replacement runtime requires bounded reader/writer execution, deadlines, and
cancellation without exposing a synchronous SQLite operation on a Tokio task.

The pure selector receives at most one candidate from `PendingNudgeStore` and
one from the task reader, then reserves zero or one `AttentionItem`. The stores
remain independent. Assignment admission creates no immediate nudge and no
ordinary message-key pending marker; only the Herdr attention selector may
choose a task reminder when the assignee is idle.

### D3. Capability inventory after Phase AZ

The optional storage capability inventory is explicitly recounted as:

1. `PeerConfigStore`
2. `OutboundMessageQuery`
3. `NudgeTemplateOverrideStore`
4. `TemplateCatalogStore`
5. `MessageSearchStore`
6. `PendingNudgeStore`
7. `TaskStore`
8. `AsyncTaskMutationStore`
9. `AttentionScheduleStore`
10. `AsyncAttentionScheduleStore`

ADR-036's inventory and the matching boundary TOMLs must be updated in AZ.2
and AZ.4. No eleventh optional capability is authorized by this ADR.

### D4. SQLite schema 2.0 major migration

Phase AZ introduces and persists `STORAGE_SCHEMA_VERSION = 2.0.0`. This is an
ADR-061 major change: stable `(team, TaskId)` identity, new state meanings and
constraints, immutable attempts, dedicated operations, and multi-task
supersession cannot be expressed as optional columns on the old
`(team, task_id, assignee)` projection without keeping two contradictory
authorities.

The major classification does not permit lockstep upgrade. For the coexistence
duration approved by Rand, the canonical v2 tables live beside the retained v1
`tasks` and `task_events` tables and `description` column. New writers update
v2 and the v1 compatibility projection transactionally. Supported writes from
the previous binary are mirrored into v2 by compatibility triggers. A retained
ATM 1.5.14 fixture must open the migrated database, assign, acknowledge, and
complete; the new binary must then reopen and observe those changes.

Migration uses `Active > Assigned > Complete` when legacy rows for one logical
task disagree, records every non-winning row, and demotes deterministic surplus
active tasks to Assigned when old data violates `(team, agent)` active
uniqueness. It never silently closes live work, reopens a fully completed task,
or makes an existing host permanently unupgradable.

Phase AZ drops no v1 object. Removing the bridge later is a separate ADR-061
major change with its own approval. Migration failure rolls back before the
version changes; successful migration remains rollback-consumable through the
coexistence bridge.

### D5. SQLite schema 2.1 additive attention migration

AZ.4 moves `STORAGE_SCHEMA_VERSION` to `2.1.0`. A registered idempotent
`DB_MIGRATIONS` step adds `attention_lane_cursors` and
`attention_opportunities` with defaults and indexes. Older consumers ignore
the additive tables. Fresh 2.1 and upgraded 2.0 databases must have
byte-equivalent schema, and the retained older-consumer suite must remain
green.

### D6. Governed-interface approval record

ADR-061 requires Rand's explicit sign-off before this ADR or the Phase AZ plan
can become Accepted/Approved. The approval record must name the 2.0.0 major
classification and the v1/v2 coexistence duration.

**Approval:** pending; no message id, issue comment, or accepted ADR citation
has yet been supplied. Planning may describe the assumed design, but
implementation must not begin and plan status must not advance until this field
is replaced with the recorded approval.

## Consequences

- Task mutation and attention scheduling gain narrow storage-neutral owners
  instead of leaking SQLite transactions into core, CLI, or runtime code.
- Capability-trait growth is explicit and capped at ten.
- The previous release remains a supported rollback consumer after migration.
- Canonical v2 rows contain no rendered body or duplicate task description;
  the retained v1 description column is a temporary, approved compatibility
  exception only.
- Cross-host terminal handoff remains outside Phase AZ because ADR-035 delivery
  cannot join the local task transaction.
- The frozen synchronous daemon remains untouched; composition is through the
  Tokio/Axum runtime and backend-neutral traits.

## Rejected alternatives

1. **Alias `TaskOperationId` to a message id.** Rejected because one operation
   can create multiple messages and events; idempotency and message identity are
   different domains.
2. **Put mutation steps in callers.** Rejected because partial message/task/
   event writes would violate the lifecycle contract.
3. **Merge message, task, and scheduler state.** Rejected because their
   lifecycle and consumption rules differ; the selector is a derived view.
4. **Drop v1 objects immediately.** Rejected because it breaks the ADR-061
   rollback consumer and forces lockstep host upgrade.
5. **Treat schema v2 as additive minor.** Rejected because identity, constraints,
   and state meaning change even while the compatibility bridge mitigates
   deployment risk.

## Required evidence

- Boundary tests for all three new sealed traits and a crate-graph proof that
  no concrete SQLite type crosses the storage boundary.
- Fresh, failed, conflicting-legacy, 1.5.14-to-2.0, and 2.0-to-2.1 migration
  fixtures, including previous-binary write/read rollback evidence.
- Atomic rollback, concurrency, idempotency, supersession,
  envelope-`taskId`-joined nudge invalidation, and no-body tests.
- Fair scheduler tests across restart with one reservation per idle opportunity
  and no merged message/task lifecycle state.
- ADR-061 version records and Rand's explicit approval citation before status
  changes from Proposed.
