---
title: Tasks
audience: end-user
reviewed_for_release: 1.6.0
---

# Tasks

ATM task mail tracks one durable task for an assignee through `assigned`,
`active`, and `complete` state (with a `completed`, `refused`, or `cancelled`
close outcome). `atm task assign` creates or updates an assigned task with
deferred notification; it never requires acknowledgement. The assignee makes
it active with `atm task start`; an assignee cannot activate a second task
while another task is active.

The assignee or assigner closes an open task with its typed outcome and a
reason or report source:

```sh
atm task close t-42 completed "implementation and verification complete"
atm task close t-42 refused --stdin
atm task close t-42 cancelled "superseded by a different task"
```

Closing is valid from either `assigned` or `active`; closing an assigned task
does not require a prior start. The retained `atm send <recipient> --task-id
<id> --task-complete --stdin` spelling remains a compatibility completion
surface, but use `atm task close` for a typed outcome. Task-linked mail never
enters the pending-ack queue.

Inspect the durable task ledger with either of these surfaces:

```sh
atm task list
atm task list --limit 50
atm task list --all --json
atm task events t-42
atm task events t-42 --limit 50
atm task events t-42 --all
atm task events t-42 --json
atm task move t-42 --head
atm task move t-42 --before t-99
atm task move t-42 --end
```

`atm task list --json` returns the caller's open task rows; `--all` includes
every team member without truncation. Both commands default to 200 rows and
accept `--limit N` or `--all`; events retain the most recent rows and display
them in sequence order. When a limit drops rows, ATM prints
`N more rows omitted (--all)` on stderr. `atm task events <id> --json` returns
an object with `events` and `handoffs` arrays. An unknown task id prints the
event header only (or `{"events":[],"handoffs":[]}` as JSON) and succeeds. See
ADR-062 for the task tables and audit/replay contract.

For a Herdr-backed assignee, ATM re-sends the Task reminder body while an open
task remains unattended: at most once per minute, after ordinary queued mail
has had its turn. A blocked assignee is not prompted, but the task event log
records the blocked reminder. Reminders stop as soon as the task is completed.

Task attention has three layers: the task reminder mail, one lead escalation
when the reminder count reaches 10, and optional configured escalation
recipients. Manage the latter with:

```sh
atm escalation add oncall@atm-dev
atm escalation add --team atm-dev oncall@ops.example
atm escalation list --team atm-dev
atm escalation remove --team atm-dev oncall@ops.example
```

Team recipients replace the daemon default for that team; otherwise the daemon
list is inherited. `atm doctor` shows the effective list and its source.

Human task rows use this stable layout:

```
pos  state     task_id     assigned_at               reminders assigner
1    active    t-42        2026-09-05T10:12:03Z      3         fenix
2    assigned  t-43        2026-09-05T10:12:04Z      0         fenix
```

Human task-event rows use this stable layout:

```
seq at event from→to actor detail
1 2026-09-05T10:12:03Z assigned -→assigned fenix -
2 2026-09-05T10:12:40Z started assigned→active cipher -
3 2026-09-05T10:13:40Z reminded active→active atm-daemon emitted
```

`atm task move` reorders only the caller's open queue; select exactly one of
`--head`, `--before <other-task-id>`, or `--end`.

Return to the [ATM User Guide](./README.md).
