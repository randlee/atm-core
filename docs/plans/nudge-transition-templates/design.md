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
R8. Started is an explicit act by the assignee: `atm task start <task-id>
    [message]`. Rand: "we need the agent to acknowledge the start of a task.
    if he doesn't acknowledge, we need to infer from events/herdr state
    instead of the agent simply saying I am starting task-1. if 'ack' is the
    mechanism to do this, it complicates ack logic a lot. I think it would be
    easier to drop ack, and simply have the agent send 'atm task start
    <task-id> <message>'." (Supersedes fenix's earlier started-on-ack call and
    amends BA design §5 "deliberately absent: start".)
R9. Ack and task are mutually independent. Rand: "Since queued task and
    --ack-required need to have different behavior, it seems they should
    simply be mutually independent so ack logic doesn't drive nudge state
    behavior." An assignment never carries an ack requirement; the queued
    message stays an ordinary unread message ("the queued task messages do
    not need to be hidden … telling an agent he needs to ack task-1/2/3
    immediately is not beneficial and is at worst distracting"). The nudge
    waits for readiness.
R10. Templates first. Rand: "the work you are planning should focus on
    template updates first. I think those are safe and we can start testing
    w/ a set of templates immediately. The logic changes should be small and
    if anything attempt to simplify things, not make them more complicated."
    Delivery plan in §9.
R11. Starting a task out of order is allowed and reorders it to head. Rand:
    "basically allowing agent to exercise judgement in a situation that
    likely requires it. it is certainly easier to allow it than to make the
    agent re-order things to do the same thing."
R12. Outcome is a field, not a template. `reassigned` is a distinct closed
    outcome on the old assignee's line. Rand: "outcome (additional state)
    info is great. since it is a field in a template, it does not multiply
    the # of templates and reassignment is distinctly different from
    cancelled."

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
| `task_ready` | task at head and assignee idle, first prompt | assignee | task pass (BA invariant) | `<atm task="{{task_id}}" ready message="{{message_id}}" from="{{from}}">`<br>`  <action>atm read --message-id {{message_id}}</action>`<br>`  <action>atm task start {{task_id}}</action>`<br>`  <action>execute the assigned task</action>`<br>`  <console announce="concise" pause="false"/>`<br>`</atm>` |
| `task_reminder` | task open, assignee idle, ≥60 s since last prompt | assignee | task pass | `<atm task="{{task_id}}" reminder="{{attempt}}" message="{{message_id}}" from="{{from}}">` + the same three actions + console |
| `task_started` | assignee runs `atm task start` (R8) | assigner | immediate, informational | `<atm task="{{task_id}}" started agent="{{assignee}}" message="{{message_id}}"/>` |
| `task_complete` | assignee closes (completed or refused) | assigner | immediate, informational | `<atm task="{{task_id}}" complete agent="{{assignee}}" outcome="{{outcome}}" message="{{message_id}}"/>` |
| `task_closed` | assigner closes (cancelled, or reassign away) | assignee | immediate, informational | `<atm task="{{task_id}}" closed by="{{from}}" outcome="{{outcome}}" message="{{message_id}}"/>` |

`message` on `task_ready`/`task_reminder` is the assignment message, so the
read action stays one command. On `task_complete`/`task_closed` it is the
close report (R7). On `task_started` it is the assignee's start message (its body is the
optional `[message]`, e.g. what the agent intends to do first). On `task_queued` it is the assignment message.

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

`reassigned` is a distinct value of the closed template's `outcome` field
(R12), not a `TaskCloseOutcome`; the task row keeps its one id and open state per BA §3.1a. The old
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
task row; it adds the transition and the attempt). `task_id` stays for the
validation arm below and for `atm read --task-id`.

### 4.2 Kind decision

```rust
match (event.is_ack, event.task_transition, event.task_id.is_some(), event.requires_ack, delivery_kind) {
    (true, _, _, _, _)                             => K::Acknowledge,
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

`BuiltInNudgeTemplateKind` gains the six kinds and drops `AcknowledgeTask`;
`"task"` and `"acknowledge_task"` parse to the same
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

### 4.4 `atm task start` (R8)

New verb, same shape as `close`:

    atm task start <task-id> [message | --stdin | --file | --template]

Caller must be the row's assignee; the row must be `assigned` (any position:
starting a non-head task is allowed and simply reorders it to head, so the
queue reflects what the agent is actually doing; R11). One writer op: the message
to the assigner with `task_op = Start`, the `assigned → active` transition,
the `Started` event, and the `task_started` receipt rendered from the same
message (immediate, informational). Idempotent on `active` ("already
started; message delivered"). Rejected on `complete`.

The daemon-written receipt (`herdr_task_start.rs`) and the
`assigned → active` transition on prompt delivery are deleted;
`complete_task_handoff` records only the reminder. A task that is prompted
but never started stays `assigned`, keeps receiving `task_reminder` with a
rising attempt, and never produces a started line: the state is inferred
from events and herdr state, never from an ack.

Ack is untouched. `atm ack` stays message hygiene, writes no task event, and
`AcknowledgeTask` is deleted along with its unreachable arm rather than made
reachable.

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

### 4.7 Ack and task independent (R9)

Today, verified: `atm send --task-id` and `atm task assign` force
`requires_ack = true` at write (`atm/src/commands/send.rs:367`; BA.4 doc:
"An assignment message always requires an ack … there is no `--requires-ack`
flag"). `pending_ack_at` is set at write. The pending-ack predicate
(`pending_nudge_store.rs:13`, `lib.rs:504-509`) has no notion of task
position, so every queued assignment is pending-ack, shown in the
`Pending-Ack:` header and listing, and claimed by the pump (SMK-006) from the
moment it is written. The BA-era decision to make the flags mutually
exclusive was lost with phase AZ; the opposite shipped.

Rule, and it is a deletion: the assignment write never sets `requires_ack`
and never sets `pending_ack_at`. The queued assignment is an ordinary unread
message; an early read marks it read normally; nothing is hidden and nothing
is deferred, because there is no ack requirement to defer. `--requires-ack`
with `--task-id` is a CLI conflict (`conflicts_with`): acknowledging a task
is `atm task start`, not `atm ack`. `atm task assign` keeps having no such
flag. No pending nudge marker is created at write (§4.3), so the pump never
claims a queued assignment; the only prompts for a task are `task_queued`
(informational) and, at head + idle, `task_ready` / `task_reminder`.

Reassign and move change `position`; nothing is migrated. `atm queue`
ephemeral items are not tasks and are unchanged (BA §9).

## 5. Findings resolved

| Finding | Resolved by |
|---|---|
| SMK-004 receipt as Task, delivered late | §3 `task_started` informational + immediate; §4.4 |
| SMK-005 no handoff record | §4.6 |
| SMK-006 non-head call-to-action + misattributed reminders | §4.3, §4.7 |
| untriaged: `AcknowledgeTask` unreachable, `Acked` never written | §4.4 deletes both |

## 6. Tests

Unit: an assignment write leaves `requires_ack` false and `pending_ack_at` NULL and creates no pending nudge marker; `--requires-ack --task-id` rejected by clap; `atm task start` by a non-assignee, on `complete`, and twice on the same task; `--requires-ack --task-id` rejected by clap; kind decision covers every arm of §4.2 including the `(false, None,
true, ..)` validation error; every default body renders with its placeholders
and contains no `<action>` for the four informational kinds; `"task"` parse
error names `task_ready`.

Integration (colima, one fixture, every roster shape): assign three tasks to
an idle agent → terminal shows exactly one `queued="2"`, one `queued="3"`,
one `ready` for task 1, no `execute` line for tasks 2 and 3; `atm read` header
shows Unread 3 / Pending-Ack 0 throughout; `atm task start` on task 1 →
assigner sees `started` with the start message id and task 1 goes `active`; close → assigner sees `complete` with the report id
and the agent sees `ready` for task 2 within one pass; task events show
`reminded attempt=0`, `acked`, `started`, `completed` on the right task ids
and nothing on the others; ready never followed by `atm task start` → `reminder="1"`, `reminder="2"` at
≥60 s, task stays `assigned`, no `started`; start of a non-head task → it
becomes head and `active`, previous head stays `assigned`; reassign → old assignee `closed outcome="reassigned"`,
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

None. R1–R12 cover every choice in this document.

## 9. Delivery plan (R10)

Sprint 1, templates only, no behavior change in WHEN anything fires:

- six kinds in `BuiltInNudgeTemplateKind`, default bodies from §3, `"task"`
  retired with a hint, `AcknowledgeTask` deleted;
- `TaskTransition` on `PostSendHookEvent`, filled from what each emit site
  already knows: the writer's task op and placement (Queued with position,
  Started for the daemon receipt until §4.4 lands, Complete/Closed from the
  close actor), the task pass (Ready when `reminder_count == 0`, else
  Reminder with the count);
- the kind decision of §4.2 and the render values;
- unit tests of §6. The ready/reminder bodies name `atm task start` from day
  one; until sprint 2 lands that verb is absent and the action line is
  advisory, which is acceptable for testing the templates.

Sprint 2, logic, each a deletion or a small addition:

- assignment write: no forced `requires_ack`, no `pending_ack_at`, no pending
  nudge marker; `--requires-ack` conflicts with `--task-id` (§4.3, §4.7);
- `atm task start` and deletion of the daemon receipt and the
  prompt-time `assigned → active` transition (§4.4);
- deletion of `queue_prompt_is_head_assignment` and
  `record_queue_prompt_reminders`; the task pass records what it emitted;
- `nudge_handoffs` (§4.6);
- colima integration tests of §6.

Sprint 1 can be dogfooded on the live team immediately after it lands via
the normal prerelease path.
