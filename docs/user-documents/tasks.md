---
title: Tasks
audience: end-user
---

# Tasks

ATM task mail tracks one durable task for an assignee through three states:
`assigned`, `active`, and `complete`. Sending mail with `--task-id` creates or
updates an assigned task; it never requires acknowledgement. The assignee
makes it active with `atm task start`; an assignee cannot activate a second task
while another task is active.

The assignee or the assigner closes an open task by sending its completion to
the assignee:

```sh
atm send cipher --task-complete t-42 --stdin
```

Completion is valid from either `assigned` or `active`. Completing an assigned
task closes it directly; task-linked mail never enters the pending-ack queue.

Inspect the durable task ledger with either of these surfaces:

```sh
atm task list
atm task list --limit 50
atm task list --all --json
atm task events t-42
atm task events t-42 --limit 50
atm task events t-42 --all
atm task events t-42 --json
```

`atm task list --json` returns the caller's open task rows; `--all` includes
every team member without truncation. Both commands default to 200 rows and
accept `--limit N` or `--all`; events retain the most recent rows and display
them in sequence order. When a limit drops rows, ATM prints
`N more rows omitted (--all)` on stderr. `atm task events <id> --json` returns
that task's append-only events. An unknown task id
prints the event header only (or `[]` as JSON) and succeeds. See ADR-062 for
the task tables and audit/replay contract.

For a Herdr-backed assignee, ATM re-sends the Task reminder body while an open
task remains unattended: at most once per minute, after ordinary queued mail
has had its turn. A blocked assignee is not prompted, but the task event log
records the blocked reminder. Reminders stop as soon as the task is completed.

Task attention has three layers: the task reminder mail, the lead escalation
at every tenth reminder, and optional configured escalation recipients. Manage
the latter with:

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
TASK_ID     STATE     ASSIGNEE  ASSIGNER   ASSIGNED_AT               REMINDERS
t-42        active    cipher    fenix      2026-09-05T10:12:03Z      3
t-43        assigned  cipher    fenix      2026-09-05T10:12:04Z      0
```

Human task-event rows use this stable layout:

```
SEQ  AT                        EVENT      FROM      TO        ACTOR    DETAIL
1    2026-09-05T10:12:03Z      assigned   -         assigned  fenix    -
2    2026-09-05T10:12:40Z      started    assigned  active    cipher   -
3    2026-09-05T10:13:40Z      reminded   active    active    atm-daemon emitted
```

Return to the [ATM User Guide](./README.md).
