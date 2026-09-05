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

Tell the team's lead when a task stalls, reserve the sender name the
daemon uses for that message, and make doctor report the roster and
task conditions an operator needs to see.

## Rule

After every successful `record_reminder` in the AX.5 task step:

```
if row.reminder_count % 10 == 0:
    lead := the single roster member of row.team with agent_type == Lead
    if lead is none or more than one: log at warn; continue          # visible via doctor
    id := write queued message from "atm-daemon" to lead, body C1, requires_ack = false, no task_id
    record_lead_notified(row, now, lead, id)
```

The message is ordinary queued mail: the lead's own pump or tmux queue
marker nudges it as today. The counter is not reset; the twentieth
reminder produces the second message.

## Deliverables

This is the authoritative deliverable checklist. Every listed deliverable
lands production-ready for the scope this sprint claims; partial or
shape-only completion fails the sprint.

- [ ] D1 — reserved sender `atm-daemon`: constant
  `RESERVED_DAEMON_SENDER: &str = "atm-daemon"` in
  `crates/atm-storage/src/contract.rs`; `add_member_with_roster_store`
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
  **inside the pump's existing `run_blocking` helper** (line 115), never
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
  one info line per team with `assigned`/`active` counts per member.
  Codes added to `crates/atm-error/src/error_codes.rs` (both match
  arms), catalog guidance in
  `crates/atm-storage/src/error_catalog.rs` (`warning_guidance`), and
  the catalog test
  `herdr_and_mixed_backend_codes_have_specific_catalog_guidance` in
  `crates/atm-storage/src/error.rs` extended to the four codes. Code
  contract C2.
- [ ] D4 — docs: `docs/requirements.md` §11.3 lists the four doctor
  codes with remediation; §12.3 (or the reserved-identifier section)
  lists `atm-daemon`; ADR-061 gains a "Lead notification" section;
  `docs/user-documents/nudge-templates.md` and `docs/team-protocol.md`
  describe the lead message.
- [ ] D5 — tests listed under Required validation.

### Paths to delete

None.

## Code contracts

### C1 — lead notification body

Sent with `atm-daemon` as `from`, `NudgeMode::Deferred`, `requires_ack ==
false`, no `task_id`:

```
task <task_id> assigned to <assignee> by <assigner> has been reminded <count> times while idle
(first reminder <first_reminded_at>, last <last_reminded_at>). Run: atm list --task-events <task_id> --member <assignee>
```

`first_reminded_at` is the `at` of the first `Reminded` row for the key.

### C2 — doctor codes and remediation

```rust
// crates/atm-error/src/error_codes.rs (additions, warning severity)
RosterNoLead,        // "ATM_ROSTER_NO_LEAD"
RosterMultipleLeads, // "ATM_ROSTER_MULTIPLE_LEADS"
RosterReservedName,  // "ATM_ROSTER_RESERVED_NAME"
TaskStalled,         // "ATM_TASK_STALLED"
```

| Code | Remediation text |
| --- | --- |
| `ATM_ROSTER_NO_LEAD` | `assign one lead: atm teams update-member <team> <member> --agent-type lead` |
| `ATM_ROSTER_MULTIPLE_LEADS` | `keep one lead: atm teams update-member <team> <member> --agent-type <other type>` |
| `ATM_ROSTER_RESERVED_NAME` | `rename the member: atm-daemon is reserved for daemon-originated messages` |
| `ATM_TASK_STALLED` | `check the assignee or close the task: atm send <assignee> --task-complete <task_id> --stdin` |

### Unchanged surfaces

`TaskStore` trait; the AX.5 reminder rule and cadence; roster schema;
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
   `ATM_TASK_STALLED`; each with the C2 remediation text.
4. `just validate` green; requirements §11.3/§12.3 and ADR-061 updated.

## Required validation

- `crates/atm-http-runtime/src/herdr_queue_wake.rs` tests (`ax6_01_*`
  naming): AC 1 scenarios, including no-lead and two-lead teams
  (no message, warn log, no `LeadNotified` row).
- `crates/atm-core/src/team_admin/member_mutation.rs` tests: AC 2 for
  add and update.
- `crates/atm-core/src/doctor/mod.rs` tests: the four codes and the
  info counts.
- `crates/atm-storage/src/error.rs` catalog test extended.
- `just validate`; quality-mgr Final Quality Report on the PR.

## Out of scope

Configurable threshold; lead escalation beyond one queued message; any
change to the reminder cadence.
