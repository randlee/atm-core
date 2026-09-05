---
phase: AX
sprint: AX.6
title: Lead notification and doctor
branch: feature/ax6-lead-notification-doctor
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ax6-lead-notification-doctor
integration_branch: integrate/phase-ax
status: draft
recommended_agent: Cipher-311d
recommended_model: fast
execution_track: C
parallel_with: []
dependency_relations:
  - prerequisite: AX.5
    dependent: AX.6
    relation: must_follow
    rationale: the lead notification fires from the reminder counter and record_reminder result AX.5 introduces in the pump.
  - prerequisite: AX.6
    dependent: AX.7
    relation: must_follow
    rationale: AX.7 proves the merged integrate head live.
---

# AX.6 — Lead notification and doctor

Tell the team's lead when a task stalls or a member is stuck at an
interactive prompt, put the same alert on the human's screen through
Herdr's notification surface, reserve the sender name the daemon uses,
and make doctor report the roster and task conditions an operator needs
to see. Doctor is a pull surface nobody is guaranteed to run (Rand,
2026-09-05: "escalating to doctor is not sufficient to get noticed"), so
every escalation in this sprint is pushed: queued mail to the lead plus a
Herdr desktop notification.

## Rule

After every successful `record_reminder` in the AX.5 task step:

```
if row.reminder_count >= 10 * (row.lead_notified_count + 1):          # due: one notification per ten reminders, none skipped
    outcome := escalate(row.team, C1 body for row)                    # lead mail + configured recipients + Herdr notification
    if outcome.lead_write is none: continue                           # no LeadNotified row; still due on the next reminder (60 s)
    record_lead_notified(row, now, outcome.lead, outcome.lead_write)  # increments lead_notified_count
```

The message is ordinary queued mail: the lead's own pump or tmux queue
marker nudges it as today. The counters are not reset; the twentieth
reminder produces the second message. Because "due" compares the two
counters, a failed write at reminder ten is retried at reminder eleven
rather than silently waiting for reminder twenty.

### Blocked escalation

Runs once per tick over the `blocked` set the AX.5 task step already
computes, whether or not the member has a task:

```
for member in blocked:
    since := blocked_since[member] or (blocked_since[member] := now)   # pump memory; entry removed when the member is no longer Blocked
    if now − since < BLOCKED_NOTIFY_MS: continue                        # 60 s: a prompt answered quickly is not an incident
    if last_blocked_notice[member] is Some and now − it < BLOCKED_RENOTIFY_MS: continue   # repeat every 10 min while still blocked
    open := tasks.open_tasks(member) if the store is available else []
    escalate(C3 body for member, since, open)
    last_blocked_notice[member] := now; stats.blocked_escalations += 1
```

### Escalation channel (shared by both rules)

```
escalate(body):
    configured := tasks.list_escalation_recipients(Team(team)); if empty: tasks.list_escalation_recipients(Daemon)   # D8 fallback
    recipients := {the single Lead of the team} ∪ configured                                # configured ATM addresses
    for r in recipients: write queued message from DAEMON_ACTOR_NAME to r, body, requires_ack = false   # per-recipient; failure logged
    herdr_process.notify(title = first line of body, body = remaining lines, deadline)                   # C4; failure logged
    log herdr_queue_poll_outcome outcome = "lead_notified" | "blocked_escalated", member, recipients count, notify ok?
```

Three layers, all pushed. **Lead**: the coordinating agent on the team.
**Escalation recipients**: one daemon-wide oversight list of ATM
addresses, optionally overridden per team (D8); a team with no list of
its own falls back to the daemon list, so one configured inbox covers
every team the daemon hosts. Each address is any resolvable form ADR-040
accepts (`agent`, `agent@team`, `agent@team.host`), so the human's chosen
relay may live on another team or another host. Each escalation is
ordinary queued mail to each address
through the same write path `atm send` uses; how that recipient reaches
a human, if at all, is outside ATM. **Screen**: the Herdr notification,
which fires even when the team has neither lead nor recipients. Nothing
here depends on anyone running doctor or reading a mailbox. `BLOCKED_NOTIFY_MS = 60_000`,
`BLOCKED_RENOTIFY_MS = 600_000`, both constants beside
`TASK_REMINDER_INTERVAL_MS`. Nothing is ever sent to the blocked agent
itself.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — reserved sender `atm-daemon`: the string is
  `atm_storage::task_store::DAEMON_ACTOR_NAME` (defined in AX.3 beside
  `TaskActor`; this sprint defines no second constant); `add_member_with_roster_store`
  and `update_member_with_roster_store` in
  `crates/atm-core/src/team_admin/member_mutation.rs` reject the name
  with `ATM_MESSAGE_VALIDATION_FAILED` and detail `atm-daemon is a
  reserved sender name`. An existing roster member with that name (from
  an older database) is not deleted; doctor reports it under
  `ATM_ROSTER_RESERVED_NAME` (D3). Verified in the cycle-1 review: the
  core write path performs no sender-membership check and no sender hook
  lookup, so no fallback is needed for a non-roster sender.
- [ ] D2 — lead notification in the pump task step
  (`crates/atm-http-runtime/src/herdr_queue_wake.rs`) per the rule; the
  write calls `write_mail_with_runtime` with `NudgeMode::Deferred`
  **inside the pump's existing `run_blocking` helper** (defined at line
  509; line 115 is one call site), never
  on the Tokio worker directly; a failed write is logged and appends no
  `LeadNotified` row (same shape as the AX.5 emit-failure rule);
  `HerdrQueueWakeStats` and the `herdr_queue_poll_tick` record gain
  `lead_notifications: usize`.
- [ ] D3 — doctor (`crates/atm-core/src/doctor/mod.rs`, reading task
  rows through `runtime.task_store()` inside
  `run_doctor_with_runtime_ports`, line 174; no new `RuntimeDoctorPorts`
  field):
  `ATM_ROSTER_NO_LEAD` warning per team with no `Lead` member;
  `ATM_ROSTER_MULTIPLE_LEADS` warning per team with more than one;
  `ATM_ROSTER_RESERVED_NAME` warning per member named `atm-daemon`;
  `ATM_TASK_STALLED` warning per open task with `reminder_count >= 10`;
  `ATM_MEMBER_BLOCKED` warning per member whose runtime observation is
  `RuntimeMemberState::Blocked` (from the doctor `runtime_status`
  snapshot), naming the member and the age of the observation;
  one info line per team with `assigned`/`active` counts per member.
  Codes added to `crates/atm-error/src/error_codes.rs` (both match
  arms), catalog guidance in
  `crates/atm-storage/src/error_catalog.rs` (`warning_guidance`), and
  the catalog test
  `herdr_and_mixed_backend_codes_have_specific_catalog_guidance` in
  `crates/atm-storage/src/error.rs` extended to the five codes. Code
  contract C2.
- [ ] D4 — docs: `docs/requirements.md` §11.3 lists the five doctor
  codes with remediation; §12.3 (or the reserved-identifier section)
  lists `atm-daemon`; ADR-061 gains a "Lead notification" section;
  `docs/user-documents/nudge-templates.md` and `docs/team-protocol.md`
  describe the lead message.
- [ ] D5 — tests listed under Required validation.
- [ ] D6 — blocked escalation in the pump
  (`crates/atm-http-runtime/src/herdr_queue_wake.rs`) per the rule:
  pump-private `blocked_since` and `last_blocked_notice` maps keyed by
  `MemberKey`, cleared when a member leaves `Blocked`; the shared
  `escalate` helper used by both rules; `HerdrQueueWakeStats` and the
  `herdr_queue_poll_tick` record gain `blocked_escalations: usize` and
  `notifications_failed: usize`.
- [ ] D7 — Herdr desktop notification through the sealed adapter:
  `HerdrProcessAdapter::notify` (code contract C4) in
  `crates/atm-herdr/src/lib.rs` beside `list` (line 203) with the real
  and fake implementations; new fixture section in
  `docs/plans/phase-aq/fixtures/herdr-cli-contract-fixture.md` with the
  argv row `["herdr","notification","show","<title>","--body","<body>","--sound","request"]`
  and its success/failure rows; dated ADR-058 amendment (second entry
  after AX.2's) adding the notification verb to the argv table and
  recording that it never targets a pane; `boundaries/atm-herdr/herdr-process-adapter.toml`
  `[contracts]` note that notification text is caller-composed and
  contains member, task id, ages and a remediation command only, never a
  message body (HR-SAFE-003 holds); `docs/atm-herdr/requirements.md`
  gains `HR-CORE-004` (notification verb, fixed argv shape, fail-soft).
- [ ] D8 — escalation recipients (code contract C5): `TaskStore` gains
  `list_escalation_recipients`, `add_escalation_recipient`,
  `remove_escalation_recipient`, each keyed by `EscalationScope::Daemon`
  or `EscalationScope::Team(name)` (trait in
  `crates/atm-storage/src/task_store.rs`, rusqlite implementation in
  `task_store.rs`, table in `shared_db.rs`). Resolution: a team's own
  list when non-empty, else the daemon list; the daemon list is the
  oversight inbox Rand configures once per host. CLI: new top-level
  `atm escalation add <address> [--team <team>]`,
  `atm escalation remove <address> [--team <team>]`,
  `atm escalation list [--team <team>]` in
  `crates/atm/src/commands/escalation.rs`; without `--team` the verbs
  act on the daemon scope. Admin functions live in a new
  `crates/atm-core/src/escalation_admin.rs` modelled on
  `set_nudge_template_override_with_store`
  (`crates/atm-core/src/team_admin.rs` line 365). The address is parsed
  at add time with the recipient parser `atm send` uses (ADR-040 forms;
  malformed → `ATM_MESSAGE_VALIDATION_FAILED`, exit 3) and resolved at
  send time like any send; an unresolvable or failed recipient is logged,
  counted in `escalation_writes_failed`, and does not stop the other
  recipients. Doctor prints the daemon list once and, per team, the
  effective list with its source (`team` or `daemon default`); no new
  warning code, since recipients are optional and the Herdr notification
  is the floor. `boundaries/atm-storage/task-store.toml`
  and the sqlite companion gain the three methods in `request_types` /
  notes. `docs/team-protocol.md` and `docs/user-documents/tasks.md`
  describe the three layers and the commands.

### Paths to delete

None.

## Code contracts

### C1 — lead notification body

Sent with `atm-daemon` as `from`, `NudgeMode::Deferred`, `requires_ack ==
false`, no `task_id`:

```
task <task_id> assigned to <assignee> by <assigner> has been reminded <count> times
(first <first_reminded_at>, last <last_reminded_at>, last outcome <outcome>: emitted|blocked|unrenderable).
Run: atm list --task-events <task_id> --member <assignee>
```

`first_reminded_at` is the `at` of the first `Reminded` row for the key.
The first line doubles as the Herdr notification title.

### C3 — blocked escalation body

```
<member> has been waiting for interactive input since <since> (<age>)
open tasks: <task_id> (assigned by <assigner>, <reminder_count> reminders) | none
Attach to its Herdr agent and answer the prompt. Run: atm members --team <team>
```

### C5 — escalation recipients

```rust
// crates/atm-storage/src/task_store.rs (TaskStore additions)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscalationScope { Daemon, Team(TeamName) }   // stored as scope_key "daemon" | "team:<name>"

/// ATM addresses that receive every escalation, verbatim as configured, for one scope.
fn list_escalation_recipients(&self, scope: &EscalationScope) -> Result<Vec<String>, AtmError>;
fn add_escalation_recipient(&self, scope: &EscalationScope, address: &str, at: IsoTimestamp) -> Result<bool, AtmError>;   // false if present
fn remove_escalation_recipient(&self, scope: &EscalationScope, address: &str) -> Result<bool, AtmError>;                   // false if absent

/// Provided method: the team's own list when non-empty, else the daemon list.
fn effective_escalation_recipients(&self, team: &TeamName) -> Result<Vec<String>, AtmError> { ... }
```

```sql
CREATE TABLE IF NOT EXISTS escalation_recipients (
    scope_key TEXT NOT NULL,      -- 'daemon' or 'team:<team_name>'
    address TEXT NOT NULL,        -- ADR-040 form as typed; resolved at send time
    added_at TEXT NOT NULL,
    PRIMARY KEY (scope_key, address)
);
```

### C4 — adapter notification verb

```rust
// crates/atm-herdr/src/lib.rs, HerdrProcessAdapter (addition)
/// Shows a Herdr desktop notification. argv is fixed:
/// herdr notification show <title> --body <body> --sound request
/// Never addresses a pane or agent; failure is HerdrError and the caller logs it.
fn notify<'a>(&'a self, title: &'a str, body: &'a str, deadline: RequestDeadline)
    -> Pin<Box<dyn Future<Output = Result<(), HerdrError>> + Send + 'a>>;
```

### C2 — doctor codes and remediation

```rust
// crates/atm-error/src/error_codes.rs (additions, warning severity)
RosterNoLead,        // "ATM_ROSTER_NO_LEAD"
RosterMultipleLeads, // "ATM_ROSTER_MULTIPLE_LEADS"
RosterReservedName,  // "ATM_ROSTER_RESERVED_NAME"
TaskStalled,         // "ATM_TASK_STALLED"
MemberBlocked,       // "ATM_MEMBER_BLOCKED"
```

| Code | Remediation text |
| --- | --- |
| `ATM_ROSTER_NO_LEAD` | `assign one lead: atm teams update-member <team> <member> --agent-type lead` |
| `ATM_ROSTER_MULTIPLE_LEADS` | `keep one lead: atm teams update-member <team> <member> --agent-type <other type>` |
| `ATM_ROSTER_RESERVED_NAME` | `rename the member: atm-daemon is reserved for daemon-originated messages` |
| `ATM_TASK_STALLED` | `check the assignee or close the task: atm send <assignee> --task-complete <task_id> --stdin` |
| `ATM_MEMBER_BLOCKED` | `<member> is waiting for interactive input; attach to its Herdr agent and answer the prompt` |

### Unchanged surfaces

`TaskStore` methods introduced by AX.3 (this sprint adds the three C5
methods); the AX.5 reminder cadence and pseudo-rule (this
sprint adds `record_lead_notified` and `list_task_events` calls inside
the task step, which AX.5 property 6 already permits); roster schema;
`AgentType` enum; every existing doctor code.

## Acceptance criteria

1. Ten reminders on one task produce exactly one message to the lead;
   the twentieth produces the second; a team without a lead produces
   none and doctor warns `ATM_ROSTER_NO_LEAD`.
2. `atm teams add-member atm-dev atm-daemon ...` and `update-member ...
   atm-daemon` exit 3 with the reserved-name detail.
3. Doctor on a team with two `lead` members warns
   `ATM_ROSTER_MULTIPLE_LEADS`; with a member named `atm-daemon` warns
   `ATM_ROSTER_RESERVED_NAME`; with an open task at ten reminders warns
   `ATM_TASK_STALLED`; with a member observed `blocked` warns
   `ATM_MEMBER_BLOCKED`; each with the C2 remediation text. Ten
   `blocked` reminders produce the lead message with `last outcome
   blocked`.
4. `just validate` green; requirements §11.3/§12.3, ADR-061, ADR-058
   amendment and `HR-CORE-004` updated.
5. A member observed `blocked` for 60 s produces exactly one lead message
   with the C3 body and one `notify` call with the C4 argv; still blocked
   at 5 min produces nothing more; at 10 min a second pair; returning to
   idle and blocking again starts a new episode. With no lead the
   `notify` call still happens and the log records `lead present = false`.
6. Every task lead notification (AC 1) is accompanied by one `notify`
   call; a failing `notify` is counted in `notifications_failed` and does
   not affect the mail or the `LeadNotified` row.
7. With one lead and a daemon-scope recipient `ops@hermes.rand-m4`, each
   escalation writes two queued messages, the second addressed through
   the normal cross-host path; a team-scope recipient replaces (not
   augments) the daemon list for that team and other teams keep the
   daemon list; with recipients and no lead, one message per recipient
   and doctor still warns `ATM_ROSTER_NO_LEAD`; a failed write to one
   recipient does not stop the others and increments
   `escalation_writes_failed`; `atm escalation add 'not an address'`
   exits 3; add/remove/list round-trip in both scopes.

## Required validation

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests (`ax6_01_*`
  naming): the AX.5 property-6 test extended to assert that
  `record_lead_notified` and `list_task_events` are the only additional
  `TaskStore` calls and that no state transition occurs; AC 1 scenarios,
  including no-lead and two-lead teams
  (no message, warn log, no `LeadNotified` row).
- `crates/atm-core/src/team_admin/member_mutation.rs` tests: AC 2 for
  add and update.
- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests (`ax6_02_*`):
  AC 5 and AC 6 with a fake adapter recording `notify` argv; a member
  blocked 30 s then idle produces nothing.
- `crates/atm-herdr` tests: `notify` argv matches the fixture row
  verbatim; a non-zero exit maps to `HerdrError`.
- `crates/atm-storage-rusqlite/src/task_store.rs` tests: recipient
  add/remove/list per scope, duplicate add returns false,
  `effective_escalation_recipients` falls back to daemon only when the
  team list is empty.
- `crates/atm-core/src/escalation_admin.rs` tests: malformed address
  rejected, ADR-040 forms accepted verbatim, `--team` absent means
  daemon scope.
- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests (`ax6_03_*`):
  AC 7 recipient fan-out with a recording write path.
- `crates/atm-core/src/doctor/mod.rs` tests: the five codes and the
  info counts.
- `crates/atm-storage/src/error.rs` catalog test extended.
- `just validate`; quality-mgr Final Quality Report on the PR.

## Out of scope

Configurable thresholds; escalation channels owned by the daemon beyond
lead mail, configured-recipient mail and the Herdr notification (how a
recipient reaches its human is that recipient's business, not ATM); notifications
for non-Herdr backends (they have no blocked signal); any change to the
reminder cadence.
