# Task-transition nudge templates — design

Status: DRAFT for Rand's approval. Author: fenix, 2026-09-12. Source: Rand's
rulings 2026-09-12 (this session), the nudge-test evidence of 15:27–15:35Z, and
source verified at origin/develop `281e6f546`.
Builds on `docs/plans/phase-ba/nudge-task-design.md` (approved, shipped). It
changes what a nudge SAYS and when the informational ones are written; it does
not change the BA invariant (an idle agent holding an incomplete task is
nudged) or the task queue model.

## 0. The two problems this exists to fix

Rand, verbatim:

  a) "tell exactly what is going on (better observability)"
  b) "diagnose issues easier"

"currently all task assignments and nudges present using the same template.
this is a bit confusing and makes it difficult to know what state transition
is occurring, particularly for team-lead who is getting nudges when
transitions occur."

## 1. What is wrong today (verified)

One function chooses the built-in nudge kind for every backend:
`built_in_nudge_template_kind_from_post_send_event`
(`crates/atm-core/src/boundary/mod.rs:148-167`). It sees `is_ack`,
`task_id.is_some()`, `requires_ack` and steer-vs-queue. A present task id wins
over everything except `is_ack`. The task operation is not on
`PostSendHookEvent` (`boundary/mod.rs:113-133`). Consequences, each observed in
Rand's 3-task nudge test on prerelease 1.5.16:

| # | Transition | Recipient | Renders as | Terminal shows |
|---|---|---|---|---|
| 1 | assignment written | assignee | `Task` | `<task id>` + summary, "ack", "execute the assigned task" |
| 2 | idle reminder (≥60 s) | assignee | `Task`, same message id as #1 | identical to #1, full body instead of summary |
| 3 | `task_started` receipt | assigner | `Task` | `<task id="X">task_started:X</task>`, "ack", "execute" |
| 4 | close report | assigner | `Task` | `<task id>` + report summary, "ack", "execute" |
| 5 | assigner cancels | assignee | `Task` | "execute the assigned task" for a cancelled task |

Three defects underneath, already triaged on PR #1431 (held):

- SMK-004: the `task_started` receipt to the assigner is a deferred queue item
  rendered with the `Task` template. It is delivered at the assigner's next
  idle moment (`herdr_queue_wake.rs:876` only claims for `Idle` members), so a
  busy assigner receives "execute the assigned task" for a task that may
  already be closed. Observed 15:30Z: receipt for `nudge-test-2` delivered
  after task-2 had closed at 15:29:54.
- SMK-005: no record is written when a nudge is handed off. Neither
  `atm log filter`, `--source timeline`, nor the launchd stderr log shows a
  claim, steer, or re-prompt. Reconstructing 15:27–15:35Z needed direct reads
  of `mail_message_states` and `task_events`.
- SMK-006: queued non-head assignments nudge the assignee immediately with the
  full call to action. The mail claim (`pending_nudge_store.rs`
  `claim_next_pending`, `ORDER BY message_key`) ignores task position; only the
  Started handoff checks head (`task_pass.rs` `queue_prompt_is_head_assignment`).
  Cipher's three assignments written 15:27:32.9 were nudged at 15:27:34,
  15:27:54, 15:28:09; cipher acked each within 6 s, the acks for tasks 2 and 3
  saying "remains queued" (the agent working around the template). Each of
  those prompts was then recorded as a `reminded` event on the HEAD task
  (`record_queue_prompt_reminders`, `task_pass.rs:105-146`): task-1 shows three
  reminders in 35 s, below the 60 s interval, while tasks 2 and 3 show none.
  The event log was faithful and misleading at once.

Also found, not yet triaged: `AcknowledgeTask` has no producer (ack writes
carry `task_id: None`, `write/acknowledgement.rs:340`), and the `Acked` task
event kind has a parser and a test row but no writer. An acked assignment
leaves no trace in the task history.

## 2. Rulings (Rand, 2026-09-12)

R1. Compound operations (reassign, reorder, reopen) are reduced to elemental
    transitions for nudging. No template per compound op.
R2. Four transitions matter: task queued, task ready (agent idle, task at
    head), task complete (assignee reports), task closed (assigner closes).
    Each may notify assigner and/or assignee.
R3. Ready and complete are the two that matter first.
R4. Informational nudges are one line. The assigner gets
    `<atm task="ID" started agent="X"/>` when work begins and
    `<atm task="ID" complete agent="X" …/>` when it ends.
R5. The assignee's ready nudge must say ready:
    `<atm task="ID" ready message="MID">`.
R6. The first ready nudge and the subsequent reminders are separate templates
    even though they carry the same information: "the subsequent 'nag' nudges
    are important, but they are also a symptom of a problem."
R7. A completion carries the message id of the completion report.
R8. (fenix's call, recorded, Rand may veto) "started" means ACKNOWLEDGED, not
    delivered: the assignee's `atm ack` of the ready nudge writes the `Acked`
    and `Started` events and triggers the started receipt. Delivery of the
    ready nudge stays recorded as `reminded` attempt 0. An unacked task then
    shows as reminders with no started line, which is exactly the symptom R6
    wants visible.

## 3. Transitions and templates

`Task` is retired. Six task kinds replace it. Escalations to leads keep
`Delivery` (out of scope, see §9). Every body below is the built-in default;
per-team overrides keep working per kind (`atm teams set-nudge-template`).

Nudge bodies are agent prompts, not parsed XML. Bare attributes (`ready`,
`started`, `complete`, `closed`, `queued`) are intentional: the transition is
the first thing the eye lands on after the task id.

| Kind | Trigger | Recipient | Mode | Default body |
|---|---|---|---|---|
| `task_queued` | assignment written and NOT at head, or reassign/reopen into a non-head position | assignee | immediate, informational | `<atm task="{{task_id}}" queued="{{position}}" message="{{message_id}}" from="{{from}}"/>` |
| `task_ready` | task at head and assignee idle, first prompt | assignee | task pass (BA invariant) | `<atm task="{{task_id}}" ready message="{{message_id}}" from="{{from}}">`<br>`  <action>atm read --message-id {{message_id}}</action>`<br>`  <action>ack the message</action>`<br>`  <action>execute the assigned task</action>`<br>`  <console announce="concise" pause="false"/>`<br>`</atm>` |
| `task_reminder` | task open, assignee idle, ≥60 s since last prompt | assignee | task pass | `<atm task="{{task_id}}" reminder="{{attempt}}" message="{{message_id}}" from="{{from}}">` + the same three actions + console |
| `task_started` | assignee acks the ready/reminder message (R8) | assigner | immediate, informational | `<atm task="{{task_id}}" started agent="{{assignee}}" message="{{message_id}}"/>` |
| `task_complete` | assignee closes (completed or refused) | assigner | immediate, informational | `<atm task="{{task_id}}" complete agent="{{assignee}}" outcome="{{outcome}}" message="{{message_id}}"/>` |
| `task_closed` | assigner closes (cancelled, or reassign away) | assignee | immediate, informational | `<atm task="{{task_id}}" closed by="{{from}}" outcome="{{outcome}}" message="{{message_id}}"/>` |

`message` on `task_ready`/`task_reminder` is the assignment message, so the
read action stays one command. On `task_complete`/`task_closed` it is the
close report (R7). On `task_started` it is the assignment message the ack
resolved. On `task_queued` it is the assignment message.

Informational kinds carry no `<action>`; the mailbox row is the record and
`atm read --message-id` works on every `message` attribute. They are never
deferred, so they cannot arrive after the fact (fixes SMK-004).

### 3.1 Compound operations reduced (R1)

| Operation | Old assignee | New assignee |
|---|---|---|
| reassign open task | `task_closed outcome="reassigned"` | `task_queued` or, if head and idle, `task_ready` on the next task pass |
| reopen closed task | — | `task_queued` / `task_ready` |
| move (`--head`, `--end`, `--before`) | nothing. `atm task list` shows position; a move to head while idle becomes `task_ready` on the next pass | — |
| cancel by assigner | `task_closed outcome="cancelled"` | — |

`reassigned` is a new `TaskCloseOutcome`-shaped value on the closed template
only; the task row keeps its one id and open state per BA §3.1a. The old
assignee's mailbox row is the informational message itself.

## 4. Contract changes

### 4.1 `PostSendHookEvent`

Add one field:

```rust
pub task_transition: Option<TaskTransition>,
```

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "transition")]
pub enum TaskTransition {
    Queued { position: u32 },
    Ready,
    Reminder { attempt: u32 },
    Started,
    Complete { outcome: TaskCloseOutcome },
    Closed { outcome: TaskClosedOutcome }, // cancelled | reassigned
}
```

Populated by the writer's post-write snapshot for Queued/Started/Complete/
Closed (the writer knows `task_op` and the placement result) and by the task
pass for Ready/Reminder (`nudge_dispatch.rs:158-189` already rebuilds from the
task row; it adds the transition and the attempt). `task_id` stays, so
`AcknowledgeTask` and the legacy `Task` fallback below remain typed.

### 4.2 Kind decision

```rust
match (event.is_ack, event.task_transition, event.task_id.is_some(), event.requires_ack, delivery_kind) {
    (true, _, true, _, _)                          => K::AcknowledgeTask,
    (true, _, false, _, _)                         => K::Acknowledge,
    (false, Some(Queued{..}), _, _, _)             => K::TaskQueued,
    (false, Some(Ready), _, _, _)                  => K::TaskReady,
    (false, Some(Reminder{..}), _, _, _)           => K::TaskReminder,
    (false, Some(Started), _, _, _)                => K::TaskStarted,
    (false, Some(Complete{..}), _, _, _)           => K::TaskComplete,
    (false, Some(Closed{..}), _, _, _)             => K::TaskClosed,
    (false, None, true, _, _)                      => unreachable after 4.3; rejected task links are stripped before the event (`send/delivery_persistence.rs:85-90`) so this arm is a validation error, not a template
    (false, None, false, false, NudgeKind::Steer)  => K::Delivery,
    (false, None, false, true,  NudgeKind::Steer)  => K::DeliveryAck,
    (false, None, false, false, NudgeKind::Queue)  => K::Queue,
    (false, None, false, true,  NudgeKind::Queue)  => K::QueueAck,
}
```

`BuiltInNudgeTemplateKind` gains the six kinds; `"task"` parses to the same
retired-kind error `delivery_task` gets today
(`crates/atm-storage/src/contract.rs:157-178`), with the hint naming
`task_ready`. Existing `task` override rows are reported by `atm doctor` and
ignored (never silently re-mapped: an override written for "execute the
assigned task" would be wrong on an informational kind).

Render values gain `position`, `attempt`, `assignee`, `outcome`, `by`
(`nudge_template.rs:60-75`); unknown placeholders stay a validation error.

### 4.3 Assignment write

Today the assignment is a deferred write whose pending mail marker the queue
pump claims in message-id order (the SMK-006 root cause). Change: the
assignment write creates NO pending mail marker. At write time it emits
`task_queued` (immediate, informational) when the row lands at a non-head
position, and nothing when it lands at head: the task pass owns `task_ready`
on the next tick, for every backend, rebuilt from the task row exactly as
reminders are today. `queue_prompt_is_head_assignment` and
`record_queue_prompt_reminders` go away; the task pass records
`reminded` with the attempt it emitted, against the task it emitted for.

`atm send --task-id` and `atm task assign` already share
`SendCommand::for_task` (`atm/src/commands/send.rs`, `task.rs:259-268`); both
get this behavior.

### 4.4 Started on ack (R8)

`atm ack` of a message whose envelope carries `task_id` writes, in the same
transaction: `Acked` task event; if the task is `assigned`, the
`assigned → active` transition and `Started`; then the `task_started` receipt
to the assigner, immediate. The ack reply envelope carries `task_id`, which
makes `AcknowledgeTask` reachable for the first time. The daemon-written
receipt in `herdr_task_start.rs` is deleted; `complete_task_handoff` records
only the reminder.

An assignee that never acks stays `assigned`, keeps receiving
`task_reminder` with a rising attempt, and never produces a started line.

### 4.5 Close report

`atm task close` / `atm send --task-complete` keep their recipient rule
(`task_close.rs:23-29`): assignee closing → assigner gets `task_complete`;
assigner closing → assignee gets `task_closed`. Both immediate, both carry the
report message id (R7). A close on an already-closed task keeps today's
"report delivered" behavior with the task link stripped; it renders `Delivery`.

### 4.6 Observability (SMK-005, ruling (a))

One durable record per emitted nudge, written by whichever path emitted it:

```
nudge_handoffs(team, agent, message_key, kind, task_id NULL, attempt NULL,
               trigger, at)
trigger ∈ steer | queue_claim | task_pass | recovery_sweep
```

Surfaced by `atm log filter --task <id>` and `atm task events <id>`
(reminders show attempt; started shows the acking message). With this table
the 15:27–15:35Z test reads as one query.

## 5. Findings resolved

| Finding | Resolved by |
|---|---|
| SMK-004 receipt as Task, delivered late | §3 `task_started` informational + immediate; §4.4 |
| SMK-005 no handoff record | §4.6 |
| SMK-006 non-head call-to-action + misattributed reminders | §4.3 |
| untriaged: `AcknowledgeTask` unreachable, `Acked` never written | §4.4 |

## 6. Tests

Unit: kind decision covers every arm of §4.2 including the `(false, None,
true, ..)` validation error; every default body renders with its placeholders
and contains no `<action>` for the four informational kinds; `"task"` parse
error names `task_ready`.

Integration (colima, one fixture, every roster shape): assign three tasks to
an idle agent → terminal shows exactly one `queued="2"`, one `queued="3"`,
one `ready` for task 1, no `execute` line for tasks 2 and 3; ack of task 1 →
assigner sees `started`; close → assigner sees `complete` with the report id
and the agent sees `ready` for task 2 within one pass; task events show
`reminded attempt=0`, `acked`, `started`, `completed` on the right task ids
and nothing on the others; unacked ready → `reminder="1"`, `reminder="2"` at
≥60 s and no `started`; reassign → old assignee `closed outcome="reassigned"`,
new assignee `queued` or `ready`; move to head while idle → `ready` next pass
and no extra line; assigner cancel → assignee `closed outcome="cancelled"`;
disabled `task_reminder` override → no nag, handoff record absent, `doctor`
reports it; busy assigner across an entire assign→close cycle → receives
`started` and `complete` at write time, never a deferred item afterwards.

## 7. Out of scope / kept

- Lead escalations (stalled, refusals, blocked, offline) stay `Delivery` with
  the `escalation:` summary. A `task_escalated` kind is a follow-up once the
  six above are in.
- The BA reminder interval (60 s), the stalled threshold (10), and the
  disposition table are unchanged.
- `atm queue` and plain `atm send` are unchanged.

## 8. Open for Rand

- R8 started = acknowledged (fenix's call). Veto → started on delivery, and
  §4.4 shrinks to writing `Acked` only.
- `reassigned` as a closed outcome on the old assignee's line (§3.1).
