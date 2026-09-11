# Task + Nudge Design — Rand's rulings, consolidated

Status: DRAFT for Rand's review. Work is STOPPED until he approves.
Author: fenix. Source: Rand's rulings 2026-09-10, verified against origin/develop
and origin/integrate/phase-az.

## 0. The two problems this exists to fix

Rand, verbatim:

  a) "agent stops for no acceptable reason."
  b) "team-lead/orchestrator interrupts current tasks in process which wreck
     context by diverting an agent to a different task/branch."

Everything below is judged against those two. Anything that does not serve them
is out.

## 1. The invariant (replaces all edge detection)

  "Up until the point a task is complete the agent MUST keep working on it."

This is an INVARIANT, not an edge. An idle agent holding an incomplete task is
out of compliance whenever observed -- not only at the instant it became true.
Nothing tracks that a transition occurred, so there is no false->true predicate,
no target generation, no revision revalidation, no edge coalescing.

Three arms, by agent state:

| agent state | has incomplete task | action |
|---|---|---|
| idle    | yes | nudge (subject to rate limit, section 6) |
| idle    | no  | nothing |
| active  | any | LEAVE ALONE -- never divert (problem b) |
| blocked | any | escalate immediately, ZERO nudges |
| dead/offline | any | escalate immediately, ZERO nudges |

Blocked and dead are not "nudge less." A nudge cannot clear a permission prompt
or revive an exited process, so every nudge is pure noise that buries the reason.

## 2. Agent state

SSOT is ephemeral state in the roster, mirrored from herdr. Already modeled:
`HerdrAgentStatus`, `HerdrError::AgentBlocked`, `RuntimeMemberState`,
`AtmErrorCode::MemberBlocked`, doctor finding at `doctor/mod.rs:771-784`.

`RuntimeMemberState` variants: Active, Idle, Offline, Unknown, IdentityConflict,
Blocked.

DO NOT consume `PickerMemberStatus` in the nudge path. It collapses
Offline|Unknown|IdentityConflict|Blocked -> Dead (`picker_projection.rs:76`).
That collapse destroys the distinction that decides remediation:
  - blocked -> a human can clear it in place
  - offline -> process is gone; work must be closed and re-created elsewhere
The nudge path consumes exact `RuntimeMemberState`.

CORRECTION (SOLAR-BA-005, verified on origin/develop). An earlier draft of this
section claimed `atm-http-runtime` has zero references to Blocked and nudges a
frozen agent every 60s. That was FALSE. The wiring exists today:

  - `herdr_queue_wake.rs:505-512` admits `Idle | Blocked` and sets
    `TaskCandidate.blocked`
  - `herdr_queue_wake_reminders.rs:123-132` records `ReminderOutcome::Blocked`
    and returns WITHOUT emitting a nudge
  - `herdr_queue_wake_reminders.rs:72-79` then calls `escalate_blocked`

So arm 4 of the invariant (blocked -> escalate, zero nudges) is substantially
built. What remains unverified is the escalation's terminal behaviour, not its
existence. Do not dispatch this as new work, and do not describe it as a gap.

The one real lossy collapse stands: `picker_projection.rs:76` folds
Blocked/Offline/Unknown/IdentityConflict into `Dead`. That projection must
never become the nudge path's eligibility authority. `atm-http-runtime` has
zero references to `picker_projection` / `PickerMemberStatus` today; the
requirement is to keep it that way.

## 3. Task identity and storage

CORRECTION (SOLAR-BA-019, verified on origin/develop at 143355254, PR #1381 /
issue #1378). Statements below that a completion report "opens a second row"
describe behaviour that has since been fixed. `apply_task_completion`
(`writer/task_ops.rs:295-338`) now resolves the existing row first — sender,
then recipient as fallback — errors when none exists, and only UPDATEs; it
never inserts a completion-side row. **Completion reports no longer manufacture
mirrors.**

Two things survive that fix and still justify this section:

  - the 14 existing duplicate groups are migration debt that #1381 does not
    clean up
  - `PRIMARY KEY (team, task_id, assignee)` still *permits* ambiguous identity;
    #1381 closed the one writer that exercised it, not the shape

So the phase's job here is narrower than first drafted: clean up existing
duplicates, and make the invariant structurally unrepresentable rather than
merely unexercised.

- ATM resolves nothing but a task-id. The task body lives in a md file, a bead,
  or a ttl file today. ATM never dereferences it.
- Therefore: NO TaskProvider trait, no resolved-provider token, no
  exactly-one-provider rule, no unresolved/ambiguous error taxonomy. All of that
  is withdrawn.
- ATM records: task-id, who, state, timestamps, event log.

### 3.1 One agent, one task (max)

develop's key IS the multi-assignment defect:

    CREATE TABLE tasks (... PRIMARY KEY (team, task_id, assignee));

Assignee is part of identity, so one task-id CAN have N rows -- one per person
it touches. That shape is the phantom mirror. Live evidence:
FIX-PRERELEASE-R3-20260909T045045Z has a completed row and a 587-reminder row
under one id; the nudged row was already done on its twin and was structurally
uncloseable.

The writer that produced those mirrors is already fixed -- see the correction
above. What remains is the 14 duplicate groups already in the ledger, and a key
that still PERMITS the shape.

There is no uniqueness on open tasks per agent on develop -- only the non-unique
index `tasks_open_by_member`. The sole one-active guard is application-level, in
`admit()` (`task_state.rs:139-148`), fires only on the `Acked` event, and its
failure mode is to reject the MESSAGE ack. That is the ack/task conflation.

SALVAGE FROM PHASE AZ -- two DDL lines, not the phase:

    PRIMARY KEY (team, task_id)                        -- mirror unrepresentable
    CREATE UNIQUE INDEX one_active_task_per_agent
        ON tasks_v2(team, current_assignee) WHERE state = 'active';

The partial index is on 'active' only. An agent may hold many 'assigned' rows.
That IS the queue: assigned = queued, active = working, one active max. The
constraint and the queue are the same structure.

RE-PROVE UNDER THE NEW KEY: assignee-reports-completion-to-assigner already
resolves the single existing row on develop (sender first, then recipient).
Under (team, task_id) the fallback lookup no longer needs an assignee at all.
The behaviour is not in question; the regression test is still required.

### 3.2 Retention

Nothing deletes task rows on develop. No `DELETE FROM tasks`, no
`DELETE FROM task_events`, no prune or retention path. An id that ever existed
always resolves.

CONSTRAINT: section 5.2's "unknown id -> error" is only safe while this holds.
If anyone adds task GC, that rule silently becomes wrong.

## 4. Task lifecycle

- assign -> queued (state: assigned)
- start  -> working (state: active); MUST notify the assigner (Rand: "when a
  task is started, a message needs to be sent letting the sender know")
- close  -> leaves the queue (no more nudges) AND records a timestamped event

Dequeue and the event record are TWO facts, not one. That separation is the
mirror fix independent of the key change: if dequeue keys on task-id rather than
on the row, a completed task cannot keep nudging from a twin.

Outcomes are TYPED, not free text, because oversight must tell them apart:

    completed | refused | cancelled | reassigned

Rationale: reassignment is close-and-create, so the outcome is the only thing in
the ledger distinguishing a finished task from an abandoned one. Free text
cannot be counted.

NOTE: the transition table already enforces close-and-create --
`(Some(Complete), Assigned) => Err("already complete; use a new id")`.

### 4.1 "blocked" is two different things -- never use one word

| sense | belongs to | word | status |
|---|---|---|---|
| agent frozen on ask-user-question / permission-request | agent state | `blocked` | fully modeled already |
| agent examined the task and cannot do it (prereqs unmet) | task outcome | `refused` | does not exist |

The codebase already owns Blocked/MemberBlocked for sense 1. Reusing the word
for sense 2 guarantees the conflation returns.

GUARDRAIL: "blockers not satisfied" is a dependency statement and is the natural
seed for a dependency graph inside ATM. The reason is a typed outcome plus text
for a human. It is NEVER an input the scheduler reasons over.

### 4.2 Refusal releases the next task

Refuse is a close, so close->next (trigger b) applies with no special rule.
Corner case: an agent that can do nothing refuses its whole queue in seconds and
lead gets N reports. Guard with a consecutive-refusal count that escalates
instead of continuing to feed. One counter, same escalation path, no new state
machine. Precedent for the shape: `release_streaks` (consecutive-release backoff
for queue claims, `herdr_queue_wake.rs:92`).

### 4.3 Queue position (replaces priority)

There is NO priority and NO reordering on develop today. Ordering is strict FIFO
by assignment time, in two places that agree:

    task_sql.rs:45          ORDER BY assigned_at ASC, task_id ASC
    herdr_queue_wake.rs:857 sort by assigned_at, then task_id
    herdr_queue_wake.rs:862 Active first, else oldest Assigned

`TaskPriority` exists only on phase-az (`task_state.rs:142`), inside the v2
types being deleted. Do not cite it as existing behaviour.

Consequence today: close-and-create can only DEMOTE. A recreated task is the
newest, so it lands at the back. Lead cannot promote anything.

DECISION: priority tiers are the wrong primitive -- they express "urgent or not",
not "position 3 of 5", and having both means two things fight over one sort.
Replaced by an explicit position. One concept.

Why ordinals are cheap here: the queue is ONE AGENT's open tasks -- a handful of
rows. Renumber the whole queue in one transaction. No sparse gaps, no LexoRank,
no fractional keys. The one-agent-one-queue shape is what makes this tractable.

Rules:

  - Position is a SEPARATE COLUMN, never `assigned_at`. Reordering by rewriting
    the timestamp falsifies the audit record; timestamps are what make incident
    reconstruction possible (the 587 analysis depended on them). Sort on
    (position, assigned_at, task_id); `assigned_at` stays immutable.
  - Default position is END. FIFO remains the behaviour when nobody intervenes.
  - `--head` means NEXT UP, NOT NOW. Position governs the assigned queue only.
    The active task is never repositioned and never preempted -- otherwise
    "jump to head" silently becomes problem (b). Rand: "--head would move to
    position #2 (#1 is occupied by current active task)." If the agent has no
    active task, head is position 1.
  - Starvation is visible, not silent: `atm task list` shows the whole queue.

Command:

    atm task move <task-id> --before <other-task-id>
    atm task move <task-id> --head
    atm task move <task-id> --end

Composes with close-and-create: an opus task mis-assigned to a haiku agent is
closed (outcome=reassigned) and re-created on the right agent with
`--before`/`--head`, landing in the correct slot instead of at the back.
Position is what makes close-and-create usable for reordering at all.

## 5. Command surface

Task is managed by a CLOSED SET of `atm task` subcommands. A clap subcommand
enum is closed by construction, so this lints under ADR-001/RBP-003 with no
extra machinery. No `atm task` namespace exists today.

    atm task assign <agent> --template <j2> --vars <json> [--task-id <id>]
    atm task close  <task-id> <outcome> [reason]
    atm task move   <task-id> --before <other> | --head | --end
    atm task list                      # oversight, section 7
    atm task events <task-id>          # history

Aliases, as implied subcommands:

    atm send <agent>    --task-id <id> --template ... --vars ...
        -> atm task assign
    atm send <assigner> --task-complete --task-id <id> --template ... --vars ...
        -> atm task close (outcome=completed), carrying a MANDATORY report

Deliberately absent: start, ack, reassign, supersede, block. Start is implicit
in beginning work and produces the receipt. Reassign is close-and-create. Ack
left the task domain entirely (section 5.1).

### 5.1 ack and task are mutually exclusive, under the hood

Ack is message hygiene only and is NEVER gated on task state. Task state and
notification go through `atm task`.

This DELETES rather than repairs `admit()`'s ack refusal, and relocates
one-active enforcement to `atm task assign` admission backed by the unique index
in 3.1.

Rand: "most important is that --ack state of a task is consistent (no args
needed)."

Today's flag shape is the inverse of the target -- two flags each carrying an id,
declared mutually exclusive, with a test named
`cli_rejects_task_complete_with_task_id`:

    #[arg(long = "task-id")]                                   task_id: Option<TaskId>,
    #[arg(long = "task-complete", conflicts_with = "task_id")] task_complete: Option<TaskId>,

Target: `--task-id` names the task once; verb flags say what to do with it. That
turns a `conflicts_with` into a `requires`, and the id reads the same everywhere.

### 5.2 Close failure dispositions

Both cases already exist as distinct arms (`task_state.rs:96-117`); only the
disposition changes.

| case | arm today | disposition |
|---|---|---|
| already complete | `(Some(Complete), Completed) => Err("already complete")` | DELIVER the message, inform the caller |
| never existed | `(None, Completed) => Err("no open task {id} for {actor}")` | BLOCK, error to caller |

ORDERING RULE: deliver the message first, then apply the close. A close that
fails must never destroy the report. Today the opposite happens -- a
`--task-complete` against a complete task rejects the whole send with
`MessageValidationFailed` and discards the body (observed first-hand
2026-09-10).

## 6. Rate limit and escalation

Both constants already exist:

    TASK_REMINDER_INTERVAL_MS      = 60_000   (herdr_queue_wake.rs:44)
    TASK_STALLED_REMINDER_THRESHOLD = 10      (task_store.rs:22)

Escalation to lead ALSO already exists (`maybe_escalate_task`,
`EscalationKind::LeadNotified`) -- but the threshold is multiplicative:

    let threshold = row.lead_notified_count.saturating_add(1)
        .saturating_mul(TASK_STALLED_REMINDER_THRESHOLD);   // 10, 20, 30, ...
    if row.reminder_count < threshold { return; }

So it fires at 10, 20, 30 forever and NEVER STOPS THE NUDGING. On the
587-reminder task, lead was notified ~58 times while nudges continued ~10 hours.
Escalation is present and built as noise instead of signal.

FIX: make the threshold TERMINAL, not multiplicative. At 10, escalate ONCE and
STOP nudging. Nudging resumes only on a state change or a reassignment. Ten
minutes per stalled task, one report. Not 587.

### 6.1 Escalation is a message, not a new structure

Do NOT persistently nudge lead -- that recreates the 587 problem one level up.

  - ONE notification per transition (agent blocked; agent dead; task stalled).
  - That notification is an ordinary message. It persists in lead's mailbox
    until read/acked. The mailbox IS the non-decaying record; nothing new is
    built to hold it.
  - Re-notification falls out of the SAME invariant applied to lead:
    lead idle + unresolved work = nudge lead; lead busy = do not interrupt.

REJECTED (fenix over-built, Rand corrected): modelling an escalation as a task
row assigned to lead. Unnecessary -- the mailbox already provides durability,
and agent state needs no SQL backing at all.

RESIDUAL RISK, accepted: a lead that never goes idle never surfaces it. Rand has
assigned blocked/dead agents to the oversight/human layer.

OPEN: Rand has deferred detailed escalation design -- "We can discuss escalation
after we get the rest of this written down."

### 6.2 Who gets escalated to -- ALREADY BUILT

Everything Rand specified exists on develop. Do not design it again.

    CREATE TABLE escalation_recipients (
        scope_key TEXT NOT NULL,
        address   TEXT NOT NULL,
        added_at  TEXT NOT NULL,
        PRIMARY KEY (scope_key, address)
    );

    pub enum EscalationScope { Daemon, Team(TeamName) }   // "daemon" / "team:<name>"
    pub const MAX_ESCALATION_RECIPIENTS: usize = 8;

    atm escalation add | remove | list

Addresses are free-form text, so `omega-prime@hermes` works today and a future
`rand@human` needs no schema change. Daemon scope covers all teams.

Resolution (`herdr_escalation.rs:204-207`):

    struct EscalationTargets {
        lead: Option<AgentName>,   // roster
        recipients: Vec<String>,   // escalation_recipients
    }

Rand's ruling:

  - no oversight account configured -> only lead is notified
  - no lead AND no recipients -> nobody is notified. ACCEPTED, not a defect.

CORRECTION (SOLAR-BA-006, verified on origin/develop). An earlier draft said
"no lead -> NOBODY is notified" and called that current behaviour. That was
FALSE and is dangerous, because implementing it would suppress configured
oversight. What the code actually does:

  - `herdr_escalation.rs:209-243` resolves lead and recipients INDEPENDENTLY.
    `lead` is `(leads.len() == 1).then(...)`, so zero leads AND two-or-more
    leads both yield `None`.
  - `herdr_escalation.rs:289-327` writes mail to the lead only if present, then
    writes to EVERY configured recipient regardless.
  - `herdr_escalation.rs:188-199` attempts a Herdr notify independently of both.

The true empty-delivery case is: no lead, no configured recipients, and a
failed or unavailable Herdr notification. That — not "no lead" — is the case
Rand accepted. The target policy is therefore: lead and recipients are
independent delivery targets; neither suppresses the other; a team with
multiple leads falls back to recipients and must be surfaced by doctor.

Doctor already warns: `AtmErrorCode::RosterNoLead` ("has no lead member", with
remediation) and `RosterMultipleLeads`. Caveat recorded, accepted: the warning
is pull-based, so a team with no lead swallows escalations silently until
someone runs doctor.

Three escalation kinds exist: `BreakerOpened`, `LeadNotified`, `BlockedEscalated`.

FUTURE, EXPLICITLY NOT TODAY: whitelist/blacklist filtering for enterprise
multi-human oversight. It is a column on this table when wanted; the shape does
not need to change.

## 7. Oversight -- state MUST be queryable

Rand: "state MUST be queryable from the system. state of both agents (available
in herdr now and mirrored in atm roster)" -- and tasks.

Both are queryable today: `atm members` returns state + age per member;
`atm list --tasks` returns TASK_ID/STATE/ASSIGNEE/ASSIGNER/ASSIGNED_AT/REMINDERS;
`atm list --task-events <id>` returns history. These move into `atm task list` /
`atm task events`.

REQUIREMENT: queryable is not the same as stored. If task state ever moves to an
external authority, the query surface is preserved as a READ-THROUGH projection.
Oversight must not die as a side effect of an internal refactor.

### 7.1 `atm task list` is a view of the queue

Rand: "in general, `atm task list` is simply to view the queue ... reporting
agent 'blocked' is simply informational, it doesn't need sql backing."

    atm task list          # the caller's own assigned tasks, in queue order
    atm task list --all    # team-lead view: all assigned tasks, all members

Agent state (blocked, dead/offline) is shown as INFORMATIONAL context, read live
from the roster at display time. It is not persisted, not a task row, and not a
separate escalation set. If an agent is blocked, that is important for a human
reading the queue to know -- that is the whole requirement.

The REMINDERS column is already the oversight metric for problem (a): an agent
that stopped for no acceptable reason shows up as a high reminder count.
Live sample: 587, 59, 39, 21, 16, 13, 13, 11, 10, 10.

## 8. What this deletes

- the attention scheduler entirely: `attention_opportunities`,
  `attention_lane_cursors`, reservations, lane fairness cursor, dispositions,
  `PermanentlyFailed`, the prune path
- assignment attempts: `task_assignment_attempts`, `current_attempt`,
  `reassign_v2`
- `task_operations` and the `TaskOperationId` idempotency namespace
- supersession
- `breaker_escalation_gates`, `breaker_cycle_opened_at`, `breaker_failure_counts`
- `EscalationState.blocked_cursor` only (round-robin fairness across blocked
  members -- pointless once each episode is reported once)

CORRECTION (fenix listed this for deletion in error): `EscalationState`
{blocked_since, last_blocked_notice} is NOT deleted. It implements the
blocked-agent escalation Rand requires -- `blocked_since` tracks the episode,
`last_blocked_notice` + `BLOCKED_RENOTIFY_MS` is the re-notify cooldown, and
`BlockedEscalated` is raised at `herdr_queue_wake_escalation.rs:226`. What
changes is the cooldown: replaced by the mailbox model of 6.1. That also fixes a
real bug -- these are in-RAM HashMaps, so a daemon restart forgets who it
already told and re-notifies. With the notice durable in the mailbox, "have I
reported this episode" is answerable from durable state and the volatility stops
mattering.
- the two-lane scheduler (message lane vs task lane) -- see section 9
- `admit()`'s ack refusal
- the whole TaskProvider/resolution design (section 3)

Of the seven state machines governing one nudge, this leaves the roster state
and the task state.

## 9. Queued messages become ephemeral tasks

DECIDED: keep `atm queue`, model it as an ephemeral task. Removing it was
considered and REJECTED -- `atm queue` is the only existing mechanism that
delivers without interrupting, i.e. the cure for problem (b). Interrupts happen
today because `atm send` defaults to `NudgeMode::Immediate`, `atm queue` is
documented nowhere, and CLAUDE.md instructs `atm send` for assignments. The tool
exists and is unused.

CLOSE TRIGGER (was the open question): determined by the existing `requires_ack`
flag, not a new concept.

    requires_ack ? closed-on-ack : closed-on-read

WHAT KEEPS THIS CHEAP: an ephemeral item is a SCHEDULING VIEW OVER THE MESSAGE,
not a second record. No task row, no task state, no `task_events` entry. The
message's own unread/pending-ack state IS the state, and `PendingNudgeStore`
already holds it. Anything else recreates the two-authorities problem this
document exists to delete, one layer down.

The selector's queue is one ordered list drawn from two sources (open tasks,
undischarged messages). That is a SORT, not a state machine: no lane cursor, no
reservation, no fairness state. This is precisely where complexity crept in last
time.

Two properties fall out:

  - An ephemeral item goes assigned -> closed without ever entering `active`, so
    it never touches `one_active_task_per_agent`. An unread message cannot block
    task assignment; no exemption needed.
  - ORDER: discharge queued messages BEFORE taking the next task. Reading costs
    seconds, and a queued message may change what the agent should do next --
    Rand's own example, `atm queue 'please let solar know when all tasks
    complete'`, must be received before the tasks complete to be worth anything.

Deletes `AttentionLane`, the lane cursors, and the fairness/cycling logic.

NON-CONCERN (raised by fenix, dismissed by Rand): that `atm queue "go fix X"` is
an untracked task in all but name. Rand: "Official planned items will explicitly
be `atm send --task ...`. An unofficial fix that is queued is a judgement call
(and unofficial thus not a concern)."

## 10. Open questions for Rand

1. CLOSED. Escalation: terminal threshold (6), message-not-stream + mailbox
   durability (6.1), recipients already built (6.2).
2. CLOSED. Close trigger is `requires_ack ? on-ack : on-read`; `atm queue` kept
   and modelled as a scheduling view over the message (section 9).
3. CLOSED. `atm task list` is a queue view (own tasks) / `--all` (whole team);
   blocked is an informational column read live from the roster, no SQL backing.
4. Timeline: solar's block on the AZ merge.
