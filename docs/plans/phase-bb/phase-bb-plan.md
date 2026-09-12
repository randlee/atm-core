# Phase BB: task transitions you can see

| Field | Value |
| --- | --- |
| Design authority | [`../nudge-transition-templates/design.md`](../nudge-transition-templates/design.md) (PR #1432). Where this plan and the design disagree, the design wins and this plan is the defect. Rulings R1–R12 live there. |
| Follows | Phase BA (`develop` at `281e6f546`, 2026-09-12) |
| Base | `develop` |
| Integration branch | `integrate/phase-bb` |
| Sprints | 7 |
| Widest parallel wave | 3 (BB.1, BB.2, BB.3) |
| Status | plan under review (plan-scope review closed at round 3 on ab1087624; critical review by solar and quality-mgr plan review pending) |
| Exactness | every enum, struct, signature, SQL statement, template body and test name is written in its sprint doc as it lands; QA diffs source against the doc. |
| Triage seed | PR #1431 (SMK-004, SMK-005, SMK-006) becomes the phase's first `.triage` records, not a fix branch |

## 1. Task state machine — one table, every transition observable

States stay `assigned`, `active`, `complete(outcome)` (ADR-062). Events stay
`Assigned`, `Started`, `Completed(outcome)`. What changes is **who** applies
`Started` and **what each transition prints**. Every row below names the
actor, the resulting state, the durable event, and the one line the
counterparty sees. Nothing else moves a task.

| # | From | Command / trigger | Actor | To | Event row | Counterparty sees (kind) |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | ∅ | `atm task assign` / `atm send --task-id` | assigner | `assigned`, position p | `assigned` | assignee: `task_queued` position p, at every position (row 2 follows when idle) |
| 2 | `assigned`, head, assignee `Idle`, `reminder_count = 0` | task pass | daemon | `assigned` (unchanged) | `reminded` outcome `emitted` | assignee: `task_ready` |
| 3 | `assigned`, head, assignee `Idle`, ≥ 60 s since row 2 or 3 | task pass | daemon | `assigned` (unchanged) | `reminded` | assignee: `task_reminder` attempt n |
| 4 | `assigned`, any position | `atm task start <id> [message]` | assignee | `active`, moved to head | `started` | assigner: `task_started` |
| 5 | `active` | `atm task start` again | assignee | `active` (no change) | none | caller: `already started; message delivered` |
| 6 | `assigned` \| `active` | `atm task close <id> completed\|refused` / `--task-complete` | assignee | `complete(o)` | `completed` \| `refused` | assigner: `task_complete outcome=o` |
| 7 | `assigned` \| `active` | `atm task close <id> cancelled` | assigner | `complete(cancelled)` | `cancelled` | assignee: `task_closed outcome=cancelled` |
| 8 | `assigned` \| `active` | `atm task assign <other>` same id | assigner | `assigned` under `<other>` | `reassigned` | old assignee: `task_closed outcome=reassigned`; new assignee: row 1's column |
| 9 | `complete` | `atm task assign` same id | assigner | `assigned` | `reopened` | assignee: row 1's column |
| 10 | `assigned` | `atm task move` | assigner | `assigned`, new position | `moved` | nothing; a move to head while idle produces row 2 on the next pass |
| 11 | `complete` | `atm task start` | — | rejected `task <id> is already complete` | `rejected` | caller error |
| 12 | `assigned` with another task `active` | `atm task start` | assignee | rejected `<assignee> already has an active task` | `rejected` | caller error |
| 13 | any | `atm task start` by non-assignee | — | rejected `task <id> is not assigned to <caller>` | `rejected` | caller error |
| 14 | `assigned` \| `active` | `atm ack` of any message | anyone | no change | none | nothing task-related (ADR-062: ack is mail hygiene) |

Retired by this phase: the daemon-authored `Started` on prompt delivery
(BA R1), the `task_started` receipt written as deferred mail, the
`start_reminder_was_emitted` gate, the forced `requires_ack` on assignment,
the `pending_ack_at` on assignment, the pending-nudge marker on assignment,
`Task` and `AcknowledgeTask` template kinds, and every reminder recorded
against a task the prompt was not for.

## 2. Decisions carried into this plan

| id | decision | record |
| --- | --- | --- |
| R1–R12 | see design §2 | design.md, PR #1432 |
| P1 | `atm task start` is the existing `WriteRequest.task_op = TaskOp::Start` on the ordinary write route. **No new route, envelope arm or wire field.** The change is writer admission (actor = assignee, any position, no reminder gate). Resolves scope-review M1. | this plan |
| P2 | The handoff record table is named `prompt_handoffs`, not `nudge_handoffs`: `scripts/check-nudge-taxonomy.py` rejects new `nudge`-family identifiers outside its frozen inventory. | this plan; design §4.6 amended |
| P3 | Scope-review PLAN-SCOPE-001: the logic sprint is split three ways by closure type — BB.4 state machine (`atm task start`), BB.5 assignment write and task pass (deletions), BB.6 `prompt_handoffs` (schema). | this plan §3 |
| P4 | Scope-review PLAN-SCOPE-002/003/004: test-procedure pages get their own sprint doc as authority (BB.3) with deliverables, acceptance, validation, the evidence-field schema and the index-refusal contract; design.md §10 points to it. | BB.3 doc |
| P5 | Scope-review PLAN-SCOPE-006: `atm doctor` reporting of stale `task` override rows, and of disabled task-kind rows (a disabled `task_reminder` silences nags), is a BB.1 deliverable (D6), together with the store method and the `clear-nudge-template` retired-name path that make the remediation possible. | BB.1 doc; §8 |
| P6 | Scope-review PLAN-SCOPE-007: `PostSendHookEvent.task_transition` crosses the graft loopback and the `ATM_INTERNAL_NUDGE` envelope (ADR-054 (g) wire-crossing contracts). It is an optional, `#[serde(default)]` field: MINOR, `HTTP_API_VERSION` 1.7.0 → 1.8.0, both receiver crates updated in BB.1, schema-reviewer sign-off on BB.1's PR. | §6 |
| P7 | Rand 2026-09-12: orchestration templates must match the shipped `atm task` surface. The 1.5.16 fixes (dispatch with `--task-id`, close on final report) cannot wait for the phase to land, so they are BB.2 (wave 1); the `atm task start` step they gain after BB.4 is in BB.7. My call, recorded so Rand can collapse it. | §3 |
| P9 | `task_queued` is emitted at every position, including head. Design §4.3 said "nothing at head"; one rule (every assignment prints one line) is simpler than a head special case, and the idle agent gets `task_ready` on the next pass anyway. Design §4.3 amended. | this plan; BB.5 D2 |
| P8 | State machine rule for this phase: no new state, no new event kind, no new counter. A sprint that needs one is a plan defect (Rand: "all state machines stay simple w/ clear observable transitions"). | §1 |

## 3. Sprint sequence

| sprint | doc | wave | parallel | recommended |
| --- | --- | --- | --- | --- |
| BB.1 | [Transition templates and kind decision](./sprint-BB.1-transition-templates.md) | 1 | with BB.2, BB.3 | arch-ctm / deep-reasoning |
| BB.2 | [Orchestration templates at 1.5.16](./sprint-BB.2-orchestration-templates-1516.md) | 1 | **PARALLEL with BB.1** | cipher / fast |
| BB.3 | [Test-procedure pages](./sprint-BB.3-test-procedure-pages.md) | 1 | **PARALLEL with BB.1** (starts with BB.1) | cipher / fast |
| BB.4 | [`atm task start`](./sprint-BB.4-task-start.md) | 2 | — | arch-ctm / deep-reasoning |
| BB.5 | [Assignment write and task pass](./sprint-BB.5-assignment-write-task-pass.md) | 3 | — | arch-ctm / deep-reasoning |
| BB.6 | [`prompt_handoffs`](./sprint-BB.6-prompt-handoffs.md) | 4 | — | arch-ctm / deep-reasoning |
| BB.7 | [Documentation, ADRs, requirements, orchestration templates](./sprint-BB.7-docs.md) | 5 | — | cipher / fast |

BB.1 is dogfooded on the live team through the normal prerelease path as
soon as it merges to `integrate/phase-bb` (design §9): the six bodies are
visible before any logic changes.

## 4. Dependency relations

| sprint | relation | rationale |
| --- | --- | --- |
| BB.2 / BB.1 | `parallel_safe` | BB.2 owns `.claude/skills/graph-orchestration/*.j2`, `.claude/skills/codex-orchestration/*.j2` and both `SKILL.md`; BB.1 owns `crates/**` only |
| BB.3 / BB.1, BB.2 | `parallel_safe` | BB.3 owns `docs/procedures/**`, `scripts/procedures/**`, `templates/procedure-report/**`, `site/reports/procedures/**`, `.just/generate_report_index.py`, two evidence writers and their tests; no crate, no skill file |
| BB.4 | `must_follow` BB.1 (dev push) | emits `TaskTransition::Started` from the writer; BB.1 defines the enum and the kind |
| BB.5 | `must_follow` BB.4 (dev push) | both edit `herdr_queue_wake/task_pass.rs` and `herdr_task_start.rs`; BB.5 deletes `record_queue_prompt_reminders`, which BB.4 reduces first |
| BB.6 | `must_follow` BB.5 (dev push) | the `task_pass` trigger row is written from the task pass BB.5 rewrites |
| BB.7 | `must_follow` BB.6 (PR completion) and BB.2 (PR completion) | documents the shipped surface; adds the `atm task start` step to the templates BB.2 corrected |

Merge-forward trigger for `must_follow` is the parent's dev push; merge
parent → child before every dev/fix round.

## 5. Worktrees and stack

`integrate/phase-bb` is created from `develop` at phase start. Wave-1
sprints branch from it independently; BB.4 stacks on BB.1's branch, BB.5 on
BB.4, BB.6 on BB.5, each PR opened and `gh stack link --base`'d on first push
(Phase BA post-mortem rules). BB.7 branches from `integrate/phase-bb` once
BB.6 and BB.2 have merged.

## 6. ADR-061 governed interfaces

| sprint | interface | change | class |
| --- | --- | --- | --- |
| BB.1 | HTTP/peer API (graft loopback `GraftPostSendRequest`, `ATM_INTERNAL_NUDGE` envelope) | `PostSendHookEvent.task_transition: Option<TaskTransition>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`; `atm-graft` and `atm-graft-python` decode with default and ignore it; `HTTP_API_VERSION` 1.7.0 → 1.8.0 | MINOR |
| BB.1 | SQLite | none — override rows keep `kind TEXT`; stale `task` rows are reported, not migrated | none |
| BB.4 | HTTP/peer API | none — `TaskOp::Start` already exists on `WriteRequest` (P1) | none |
| BB.4 | SQLite | none | none |
| BB.5 | all three | none — deletions on the write path and the task pass | none |
| BB.6 | SQLite | new table `prompt_handoffs` via `CREATE TABLE IF NOT EXISTS` in `TASK_TABLES_DDL`; additive, a pre-BB binary ignores it | MINOR |
| BB.6 | HTTP/peer API | `TaskEventRow` gains no field; handoffs are read by `atm task events` through a new optional `handoffs` array on the existing task-events list response | MINOR, `HTTP_API_VERSION` 1.8.0 → 1.9.0 |
| BB.3 | none | evidence JSON and the report index are not governed interfaces; BB.3 pins their contract in its own doc | n/a |

`schema-reviewer` signs BB.1 and BB.6.

## 7. Requirements and ADR edits, by sprint

Rand 2026-09-12: "make sure that req/adr are updated where appropriate,
req/adr are listed in sprint plans". Every edit below is owned by exactly
one sprint and is in that sprint's deliverables.

| document | lines (at `281e6f546`) | edit | sprint |
| --- | --- | --- | --- |
| `docs/requirements.md` | 1368–1383 | "exactly seven named template cases" → the eleven kinds of BB.1 D1; `acknowledge_task` removed; "task-tagged messages select `task`" → transition selects the task kind | BB.1 |
| `docs/requirements.md` | 4971 | "seven built-in nudge template bodies" → eleven | BB.1 |
| `docs/adr/ADR-061-…` | D5 | 1.8.0 entry (BB.1); 1.9.0 entry (BB.6) | BB.1, BB.6 |
| `docs/adr/ADR-061-…` | D6 | storage-schema record: additive `prompt_handoffs` table, MINOR, no approval needed | BB.6 |
| `docs/adr/ADR-054-…` | (a) Taxonomy; new "Phase-BB amendment" | eleven built-in template kinds; the six task kinds are transition-selected; `task`/`acknowledge_task` retired; `PostSendHookEvent.task_transition` recorded under (g) as the both-sides change | BB.1 |
| `docs/requirements.md` | 2996–2997 (§15.4 item 4) | "Starting a task MUST move it … and MUST send the assigner a start notification" gains "by `atm task start` from the assignee; the daemon never starts a task" | BB.4 |
| `docs/requirements.md` | 3010–3013 (§15.4 item 9) | closed set adds `start` | BB.4 |
| `docs/adr/ADR-062-…` | 134–137 ("Phase BA R1 is decided … no public `start` command") | replaced by a "Phase BB amendment": `Started` is applied only by the assignee through `atm task start`; a prompt never transitions | BB.4 |
| `docs/requirements.md` | 1585, 1591 (§6.5) | delete "require acknowledgement for any task-linked message" and "imply `--requires-ack`"; add "`--requires-ack` conflicts with `--task-id`" | BB.5 |
| `docs/requirements.md` | 2975 (§15.4) | "every task-linked message must require acknowledgement" → "a task-linked message never requires acknowledgement; readiness is signalled by the task pass, start by `atm task start`" | BB.5 |
| `docs/requirements.md` | §15.4 item 15 | unchanged (Idle with open task nudged ≤ once per 60 s); add "the first prompt is `task_ready`, later ones `task_reminder`" | BB.5 |
| `docs/adr/ADR-062-…` | "Reminder and escalation" table | reminders are recorded against the task prompted for (never the queue head by position) | BB.5 |
| `docs/adr/ADR-054-…` | "Phase-BA amendment" | an assignment carries no pending-nudge marker; the task pass owns every task prompt | BB.5 |
| `docs/adr/ADR-062-…` | new subsection | `prompt_handoffs` is the durable record of every emitted prompt; `task_events.reminded` stays the task-side counter | BB.6 |
| `docs/team-protocol.md` | 24–41 "Task Commands"; 42–60 message classes | `atm task start`; task assignments are informational until `task_ready`; never `atm ack` a task assignment | BB.7 |
| `CLAUDE.md` | 239–247 quick reference | `Start a task` row; alias note | BB.7 |
| `docs/agent-conventions.md` | nudge section | six task lines and what each asks of the reader | BB.7 |
| `docs/adr/INDEX.md` | — | amendment rows | BB.7 |

## 8. Additions list

Nothing outside this list is added; deletions are listed per sprint.

| sprint | addition |
| --- | --- |
| BB.1 | `TaskTransition` enum; `PostSendHookEvent.task_transition`; six `BuiltInNudgeTemplateKind` variants; six default bodies; render values `position`, `attempt`, `assignee`, `outcome`, `by`; `TaskClosedOutcome { Cancelled, Reassigned }`; doctor findings `stale_nudge_template_override` and `disabled_task_nudge_template`; `NudgeTemplateOverrideStore::list_stale_template_override_kinds`; `clear_template_override(team, kind: &str)` (parameter type change, internal trait); `HTTP_API_VERSION` 1.8.0 |
| BB.2 | nothing in `crates/`; template steps only |
| BB.3 | `scripts/procedures/render_procedure_pages.py`; `templates/procedure-report/procedure.html.j2`; `docs/procedures/*.md`; `site/reports/procedures/**`; `source_revision` on two evidence writers; `procedure`/`source_revision` optional envelope fields |
| BB.4 | clap `TaskSubcommand::Start(TaskStartCommand)`; `SendCommand::build_task_start_request`; writer `admit` arm for `Started` (actor = assignee) |
| BB.5 | `conflicts_with = "task_id"` on `--requires-ack`; `MessageAdmissionOutcome.{queued_position, reassign_notice}` (internal); `task_queued` emission in the writer post-write; the reassign notice row; `TaskTransition::{Ready, Reminder}` set by the task pass |
| BB.6 | table `prompt_handoffs`; `PromptHandoff`, `PromptTrigger` types; `TaskStore::record_prompt_handoff`, `AsyncTaskLedgerReader::list_prompt_handoffs`; `handoffs` on the task-events list response; `HTTP_API_VERSION` 1.9.0 |
| BB.7 | nothing |

## 9. Phase acceptance

1. Every row of §1 is exercised by a named colima integration test (BB.4,
   BB.5, BB.6 docs) and every test passes on the integrate head.
2. `grep -rn "AcknowledgeTask\|K::Task\b\|start_assigned_task\|start_reminder_was_emitted\|queue_prompt_is_head_assignment\|record_queue_prompt_reminders" crates/` returns nothing.
3. A three-task assignment to an idle agent on the live team shows exactly
   one `queued="2"`, one `queued="3"`, one `ready` (SMK-006 closed).
4. `atm task events <id>` shows every prompt for that task with its kind,
   attempt and trigger (SMK-005 closed).
5. The `started` line reaches the assigner at write time, never later
   (SMK-004 closed).
6. Every §7 edit landed; `just lint spell`, `just lint nudge-taxonomy`,
   `schema-reviewer` sign-offs on BB.1 and BB.6.
7. Every report on `site/reports/index.html` links a procedure page (BB.3).
8. Every dispatch template assigns with `--task-id` and closes with
   `atm task close` (BB.2) and, after BB.7, starts with `atm task start`.
