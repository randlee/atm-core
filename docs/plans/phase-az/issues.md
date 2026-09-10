# Phase AZ issue inventory

Planning identifiers in this file track scope and sprint ownership. They do not
replace repository issue numbers or QA triage authority.

| Planning id | Status | Owning sprint | Required closure evidence |
| --- | --- | --- | --- |
| `AZ-LONG-NUDGE` | planned | AZ.1 | All Steer, Queue/rebuild, Task/reminder, graft, and Herdr projections use persisted title metadata; ordinary/J2 body sentinels are absent. |
| `AZ-TASK-LIFECYCLE` | planned | AZ.2 | Corrected state table, stable logical id, immutable attempts/events, priority, migration, database invariants, idempotency, and concurrency tests pass. |
| `AZ-TASK-COMPLETE-PENDING` | planned | AZ.2 | Block/close/reassign/reopen/supersede atomically join canonical messages by `(team, task_id)` and invalidate assignment, progress, requeued, and legacy task-linked pending markers. Missing historical messages are audit facts and cannot retain a marker. |
| `AZ-TASK-COMMANDS` | planned | AZ.3 | Canonical task CLI/API, authorization, explicit start, durable handoffs, supersession, and legacy adapters pass end-to-end. |
| `AZ-IDLE-INTERLEAVING` | planned | AZ.4 | One derived selector emits at most one item per idle opportunity and alternates the separate ephemeral/persistent lanes across restart. |
| `AZ-GOVERNED-INTERFACES` | planned | AZ.2/AZ.3/AZ.4 | HTTP API 1.5.0 and storage schema 2.0.0/2.1.0 carry ADR-061 records and older-consumer tests; ADR-061 D6 and ADR-063 D6 record the approved `1.6.x` coexistence window and ATM `1.7.0` as the planned and earliest permitted bridge-removal release. |
| `GH-1378-CANONICAL-AGENT-STATE` | prerequisite | pre-AZ.4 | Herdr poll and authenticated heartbeat POST converge on one ephemeral master-roster state; health/CLI are projections; the canonical state/revision seam is available for AZ.4 to mint one idempotent opportunity per accepted idle revision; failed polls and stale revisions emit nothing. |

## Binding clarifications

- The earlier shorthand `Active <-> Blocked` is superseded. Legal blocking
  transitions are `Assigned -> Blocked`, `Active -> Blocked`, and explicit
  `Blocked -> Assigned` with a resolution note. Unblock preserves priority and
  original assignment time and never starts the task.
- An idle opportunity derives one `AttentionItem`: either an ephemeral queued
  message or a persistent task reminder. Their lifecycle storage remains
  separate; only a small scheduler cursor records which lane is next.
- The opportunity source is one canonical ephemeral master-roster state, not
  raw Herdr output, a heartbeat-only transition, or `RuntimeHealth`. Herdr and
  authenticated hook/heartbeat updates share accepted-mutation ordering;
  source and timestamps are metadata only.
- A material objective change is never an in-place edit. It closes the old task
  as `Aborted(Superseded)` and creates a linked, distinct successor `TaskId`.

## Exclusions

- No Phase AZ sprint modifies the frozen legacy synchronous daemon.
- ATM does not own Beads task details; a Beads id may only be used as the opaque
  `TaskId`.
- No live daemon/test-daemon, tag, release, publish, or installation action is a
  Phase AZ validation step.

## Fenix design-review correction record

| Finding | Plan correction |
| --- | --- |
| `AZ-DES-001` | AZ.1 uses dual-key internal/graft wire DTOs, a deserialization alias, reader-first 1.5.14/1.5.15 skew tests, and an ADR-054 amendment. |
| `AZ-DES-002` | AZ.3 D5 and its path list own `crates/atm-core/src/send/mod.rs` assignment dispatch/queue suppression. |
| `AZ-DES-003` | Canonical terminal handoff is same-host only; cross-host rejects before mutation. |
| `AZ-DES-004` | The phase governed-interface matrix specifies HTTP 1.5.0 and storage 2.0.0/2.1.0 records, migrations, compatibility tests, and Rand's approved `1.6.x` bridge window. |
| `AZ-DES-005` | Typed legacy completion preserves assigner/assignee and Assigned/Active behavior without widening canonical completion. |
| `AZ-DES-006` | AZ.2 retains ack activation; AZ.3 lands the mail-only ack switch atomically with explicit start. |
| `AZ-DES-007` | All sprint validations run the line-count gate and AZ.2/AZ.3/AZ.4 authorize split modules. |
| `AZ-DES-008` | AZ.2 owns list/top-runnable ordering and the covering index. |
| `AZ-DES-009` / `AZ-DES-009-R` | Accepted ADR-063 recounts the twelve live semantic capabilities, groups required async companions per ADR-036, and assigns task mutation, attention scheduling, and schema-major rationale. |
| `AZ-DES-010` | Migration uses open/active precedence, records discarded rows, and deterministically demotes surplus active rows. |
| `AZ-DES-011` | A dedicated operations table owns idempotency; both supersession histories may reference one operation. |
| `AZ-DES-012` | Invalidation joins every canonical message envelope by `(team, taskId)`, not only attempt message ids. |
| `AZ-DES-013` | Reminder eligibility is attempt-aware while escalation ordinal remains task-scoped across attempts. |
| `AZ-DES-014` | Missing and ambiguous lead authority have distinct typed rejections. |
| `AZ-DES-015` | AZ.2 owns public error registry, catalog, boundary, and recovery documentation paths. |
| `AZ-DES-016` | AZ.2/AZ.4 authorize only deliberate frozen nudge-inventory additions beside ADR-063. |
| `AZ-DES-017` | Closed/event reads default to 200 rows; bounded `--limit` and explicit `--all` are mutually exclusive. |
| `AZ-DES-018` | Canonical and legacy task-linked mail retain requires-ack, read visibility, and clear protection. |
| `AZ-DES-019` | AZ.3 D6 updates and tests the `ATM_TASK_STALLED` recovery hint. |
| `AZ-DES-020` | AZ.1 removes Task-body wording; AZ.4 removes drain-first wording. |
| `AZ-DES-021` | AZ.4 D5 and paths include the canonical task lifecycle schema. |
| `AZ-DES-022` | Both attention lanes share `MAX_NUDGE_ATTEMPTS = 5`; permanent failure terminalizes only the reservation. |
| `AZ-DES-023` | User-facing docs distinguish lifecycle-blocked tasks from runtime-blocked members. |
| `AZ-DES-024` | AZ.4 non-closure states the selector is Herdr-only and preserves bare-CLI pull behavior. |
| `AZ-DES-025` | AZ.2 assigns `idx_mail_messages_task_id(team, json_extract(...))` to the canonical mail-index macro and its fresh/migrated identity test. |
| `AZ-DES-026` | AZ.2 reuses migration precedence and deterministic active-conflict demotion for legacy writes admitted through the v1/v2 bridge. |
| `AZ-DES-027` | AZ.2/AZ.3 apply the same same-host restriction and typed error to assign, reassign, reopen, successor assignment, and terminal handoff recipients. |
| `AZ-DES-028` | AZ.4 registers `attention_schedule_store::ensure_schema` from `shared_db::ensure_schema`; `DB_MIGRATIONS` remains a SQL batch. |
