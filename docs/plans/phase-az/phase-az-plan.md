---
phase: AZ
title: "Phase AZ: bounded nudges and durable task lifecycle"
canonical_path: docs/plans/phase-az/phase-az-plan.md
planning_branch: plan/phase-az-task-nudge-contract
integration_branch: develop
status: approved
owner: solar
authored: 2026-09-09
baseline: develop@0ca0878cf4045546fb7f4b4f14dc6c473995158c
authority:
  - arch-ctm@atm-dev message 01M23DRDJ0CXCZH49NNAZ8EA31
  - arch-ctm@atm-dev task PHASE-AZ-2-TASK-LIFECYCLE-PLANNING
  - arch-ctm@atm-dev messages 01M23GR2K5M5SEBX8ZWDJAM8Q2 and 01M23GVQMV5E3BW7F5K65JEK87
  - team-lead@atm-dev message 01M23E8CGE4ENY59CY2VT079YX
  - Rand's 2026-09-09 major-change and coexistence approval, recorded in ADR-061 D6 and ADR-063 D6
---

# Phase AZ: bounded nudges and durable task lifecycle

## Why this phase exists

ATM first needs to restore the notification boundary: nudges are bounded wake-up
metadata, while the immutable message body is retrieved only with `atm read
--message-id`. That remains the complete and unchanged scope of AZ.1.

The approved follow-up design then replaces the message-derived
three-state task ledger with a durable logical-task lifecycle. A stable `TaskId` survives
assignment attempts, explicit commands own state changes, terminal outcomes and
supersession are recorded, and the runtime derives at most one fair idle work
item from the independent ephemeral-message and persistent-task lanes.

## Binding outcomes

### AZ.1 notification boundary

The only message-derived values permitted in a nudge are `message.id`,
`message.title` sourced from persisted `MessageEnvelope.summary`, and optional
`message.task-id`. A missing title becomes empty. AZ.1 does not change task
lifecycle, task storage, queue invalidation, or admission-time `build_summary`.
The external `ATM_POST_SEND` JSON payload retains deprecated
`description = title` during one compatibility window; it is a second key for
the same bounded title metadata, never a body source.

### AZ.2+ task lifecycle

- One stable `(team, TaskId)` row identifies a logical task. Immutable assignment
  attempts and append-only events preserve history without storing rendered
  message bodies or duplicate task descriptions.
- States are `Assigned`, `Active`, `Blocked`, and `Closed`; terminal outcomes are
  `Succeeded`, `Failed`, and `Aborted`. `Aborted(Superseded)` links to a newly
  created `TaskId` when the objective materially changes.
- `Assigned -> Active` requires explicit `atm task start`. Both `Assigned` and
  `Active` may become `Blocked`. Explicit unblock records a resolution and moves
  `Blocked -> Assigned`, retaining original priority and `assigned_at`; it never
  activates the task. `Closed -> Assigned` is available only through authorized
  reopen/reassign and creates a new assignment attempt.
- Each task has exactly one current assignee. A database constraint permits at
  most one `Active` task per `(team, agent)`. All transitions, assignment-attempt creation,
  terminal handoff persistence, supersession, and task-related pending-nudge
  invalidation are atomic and idempotent.
- Existing rows migrate with `Normal` priority. Runnable order is `Active` first,
  then `Assigned` by priority and original `assigned_at`; `Blocked` sorts last
  and is not reminder-eligible.
- `atm task` is the canonical public command family. Legacy `atm send --task-id`,
  `atm send --task-complete`, `atm list --tasks`, and `atm list --task-events`
  remain temporary deprecated adapters to the same service, never alternate
  mutation/query paths. AZ.2 retains acknowledgement activation until AZ.3
  atomically lands `atm task start`; only then does acknowledgement become
  mail-only. The legacy completion adapter retains assigner-or-assignee closure
  from Assigned or Active during its compatibility window.
- Closing a task requires a durable handoff message to another roster member.
  Template plus variables is the documented default; plain text and standard
  message sources remain supported. The handoff message and closure commit
  together. Every assignment-bearing mutation (assign, reassign, reopen, and
  successor assignment) and canonical handoff resolves a same-host roster
  member; handoff additionally requires non-self. A cross-host recipient rejects
  with `ATM_TASK_HANDOFF_CROSS_HOST_UNSUPPORTED` because ADR-035 remote
  delivery cannot participate in the local task transaction.
- `TaskId` remains an opaque validated identifier and may equal a Beads id. ATM
  stores no Beads task body and does not query Beads for task details.

### One derived idle-work selector

The replacement Tokio/Axum runtime derives one `AttentionItem` per eligible idle
opportunity:

- `EphemeralMessage { message_id }` comes from the existing pending queue and is
  consumed after one nudge, or suppressed if read and acknowledged first.
- `PersistentTaskReminder { task_id, assignment_attempt,
  assignment_message_id }` comes from the open task ledger and stays eligible
  until block or closure.

The two lanes keep separate lifecycle storage and never copy message bodies.
When both are due for one agent, the selector emits the FIFO ephemeral item on
the first opportunity, the active task (otherwise top runnable assigned task)
on the next, and continues alternating. Exactly one item is emitted per idle
opportunity. Task priority orders only the persistent lane; it never jumps a
task ahead of the ephemeral lane.

This selector is specifically the Herdr idle-attention pump. It does not alter
ADR-054's bare-CLI pull behavior.

## Issue inventory

| Planning id | Disposition | Closure |
| --- | --- | --- |
| `AZ-LONG-NUDGE` | In scope | AZ.1 removes direct body/task-description fallbacks and closes every listed projection with focused negative tests. |
| `AZ-TASK-LIFECYCLE` | In scope | AZ.2 replaces the task domain/schema and provides transactional mutation primitives. |
| `AZ-TASK-COMMANDS` | In scope | AZ.3 adds the canonical command/service surface, authorization, durable handoffs, and legacy adapters. |
| `AZ-TASK-COMPLETE-PENDING` | In scope | AZ.2 atomically joins canonical messages by `(team, task_id)` and invalidates every task-linked pending marker—not only assignment-attempt ids—when a task blocks, closes, reassigns, reopens, or is superseded. |
| `AZ-GOVERNED-INTERFACES` | In scope | AZ.2/AZ.3/AZ.4 implement the ADR-061 version, migration, record, and older-consumer matrix; ADR-061 D6 and ADR-063 D6 record Rand's approval of the major storage change and its version-bounded coexistence window. |
| `AZ-IDLE-INTERLEAVING` | In scope | AZ.4 replaces drain-first reminder scheduling with the one-item fair selector. |

These are planning identifiers, not substitutes for repository issue numbers.

## Sprint sequence

| Sprint | Status | Branch | Authoritative plan | Production closure |
| --- | --- | --- | --- | --- |
| `AZ.1` | `planned` | `feature/az1-task-nudge-contract` | [`sprint-AZ.1-task-nudge-contract.md`](./sprint-AZ.1-task-nudge-contract.md) | bounded metadata-only nudge contract |
| `AZ.2` | `complete` | `feature/az2-task-domain-storage` | [`sprint-AZ.2-task-domain-storage.md`](./sprint-AZ.2-task-domain-storage.md) | migrated durable domain, immutable attempts/events, atomic invariants and invalidation |
| `AZ.3` | `complete` | `feature/az3-task-command-handoff` | [`sprint-AZ.3-task-command-handoff.md`](./sprint-AZ.3-task-command-handoff.md) | complete public task command/service, authorization, handoffs, compatibility adapters |
| `AZ.4` | `planned` | `feature/az4-attention-scheduler` | [`sprint-AZ.4-attention-scheduler.md`](./sprint-AZ.4-attention-scheduler.md) | fair one-item idle selector and persistent reminder lifecycle |

The split is intentionally sequential. AZ.2 changes the public task records and
storage invariants consumed by both later sprints. AZ.3 exposes only transitions
whose complete transactional semantics already exist. AZ.4 then replaces the
runtime scheduler against the final query and attempt identities. No deliverable
is repeated across sprint checklists.

## Dependency relations

- `AZ.1 must_follow develop@0ca0878cf4045546fb7f4b4f14dc6c473995158c`.
- `AZ.2 must_follow AZ.1`: AZ.1 development must be pushed before AZ.2 begins;
  merge AZ.1 parent into AZ.2 before every development/fix round, and AZ.1 PR
  must merge before AZ.2 PR completes.
- `AZ.3 must_follow AZ.2`: AZ.2 development must be pushed before AZ.3 begins;
  merge AZ.2 parent into AZ.3 before every development/fix round, and AZ.2 PR
  must merge before AZ.3 PR completes.
- `AZ.4 must_follow AZ.3`: AZ.3 development must be pushed before AZ.4 begins;
  merge AZ.3 parent into AZ.4 before every development/fix round, and AZ.3 PR
  must merge before AZ.4 PR completes.

The trigger is parent development push, not QA completion. None of these
relations is `parallel_safe`: they intersect on task contracts, SQLite schema,
runtime composition, or scheduler-visible state.

## Cross-sprint document ownership

- AZ.1 owns the metadata-only nudge amendments already enumerated in its plan.
- AZ.2 amends ADR-062, the product task requirements/architecture, storage crate
  docs, task-store boundary TOMLs, and creates the canonical
  `docs/task-lifecycle-schema.md` migration/schema contract.
- AZ.3 amends CLI/core/API requirements, architecture, help/user documents, HTTP
  API schema, and task command examples.
- AZ.4 amends runtime/Herdr requirements and architecture, pending-nudge and task
  reader boundaries, and the idle-selector runtime contract.
- Each sprint updates this phase plan, `docs/project-plan.md`, and the Phase AZ
  issue inventory only for its own status/evidence. Historical Phase AX plans
  remain historical and are not rewritten.

## ADR-061 governed-interface plan

All three managed interfaces are classified here so phase-end schema review can
diff implementation against the approved plan. Every version change, baseline,
older-consumer fixture, and ADR-061 version-record entry lands in the sprint
that owns the interface change.

| Sprint | HTTP/peer API | Herdr IPC | SQLite schema |
| --- | --- | --- | --- |
| AZ.1 | No HTTP route/DTO change; patch classification. The separate internal-nudge/graft compatibility seam follows ADR-054's reader-first dual-key plan. | Prompt content becomes bounded title metadata but the Herdr request shape and `HERDR_MINIMUM_VERSION` are unchanged; patch classification. | No schema/version change. |
| AZ.2 | No HTTP/version change. | No Herdr/version change. | **Major:** ATM `1.6.0` introduces persisted `STORAGE_SCHEMA_VERSION = 2.0.0` and canonical v2 task/attempt/event/operation tables with changed constraints/meaning. Retain the full v1 table/column projection and transactional bidirectional compatibility bridge throughout `1.6.x`; ATM `1.7.0` is the planned removal target and earliest permitted removal release, subject to a separate ADR-061 major approval. |
| AZ.3 | **Minor:** additive task routes, request variants, and optional response fields; bump `HTTP_API_VERSION` 1.4.0 → 1.5.0, update both OpenAPI documents and surface baseline, append ADR-061 D5 record, and run retained 1.4.0 consumer tests. | No Herdr/version change. | No schema/version change beyond consuming AZ.2. |
| AZ.4 | No HTTP/version change. | Scheduler implementation changes behind the existing Herdr request shape; `HERDR_MINIMUM_VERSION` remains unchanged. | **Minor:** additive attention cursor/reservation tables; bump `STORAGE_SCHEMA_VERSION` 2.0.0 → 2.1.0 through a registered idempotent migration, update schema/ADR-061 records, and run fresh/upgraded plus older-consumer tests. |

AZ.2's storage change cannot be expressed as merely optional columns: stable
logical identity collapses the old `(team, task_id, assignee)` rows, introduces
terminal sum-state meaning and one-active constraints, and must support atomic
multi-task supersession. It is therefore intentionally classified major even
though the coexistence bridge prevents lockstep upgrade.

**Approval record:** On 2026-09-09, Rand approved the schema-v2 major
classification and directed ATM `1.6.0` to introduce v2 while retaining the v1
bridge throughout `1.6.x`. ATM `1.7.0` is the planned bridge-removal target and
the earliest permitted removal release, under a separate ADR-061 major-change
approval. ADR-061 D6 and accepted ADR-063 D6 are the durable citations for that
decision.

The approved rollback story is concrete: after v2 migration, a retained ATM
1.5.14 binary opens the same database, reads and mutates its v1 task projection,
and compatibility triggers mirror supported assign/ack/complete writes into v2.
The new binary can then reopen and observe the reconciled writes. During
`1.6.x`, the legacy guarantee is no crash and no compatibility rejection for
those supported operations, not preservation of conflicting v1 semantics.
Canonical v2 behavior applies immediately. Phase AZ drops no v1 object; the
planned ATM `1.7.0` bridge removal is a separately approved ADR-061 major
change and cannot occur earlier.

## Architecture boundary

Task state policy and DTOs remain storage-neutral. SQLite owns schema,
migration, unique constraints, writer transactions, and separate task/message
lifecycle tables. `atm-core` owns command policy and API request mapping; `atm`
owns clap/output only. The Tokio/Axum `atm-http-runtime` owns idle scheduling
and depends on injected storage-neutral readers/writers. No CLI, core, or
runtime module opens SQLite directly.

The frozen synchronous daemon is excluded. Validation uses unit/integration
fixtures and temporary stores only; no live daemon, test daemon, tag, release,
publish, or installation operation belongs to any Phase AZ sprint.

## Phase acceptance

Phase AZ is complete only when all four sprint acceptance lists pass and:

1. AZ.1 nudges contain bounded persisted metadata and no direct body fallback.
2. Migrated and new tasks obey the corrected explicit lifecycle and durable
   history rules under retries and concurrent transitions.
3. Operators can perform every lifecycle transition through `atm task`, every
   terminal transition durably hands off, and legacy flags use that same path.
4. Each idle opportunity selects zero or one item, alternates fairly when both
   lanes remain due, and never reminds blocked or closed tasks.
5. No task or attempt row stores a rendered message body or duplicate
   description in canonical v2 storage, and task-related pending queue markers
   cannot survive ineligible lifecycle transitions. The invalidation join
   covers every canonical message with the same `(team, task_id)`, not only
   assignment-attempt message ids.
6. HTTP API 1.5.0 and storage schema 2.0.0/2.1.0 changes carry their ADR-061
   records, documentation, migration baselines, and older-consumer tests.
7. ADR-063 is accepted and indexed, and the ADR-061 major storage approval plus
   coexistence duration is cited before plan approval or implementation begins.
