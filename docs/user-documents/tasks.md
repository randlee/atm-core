---
title: Tasks
audience: end-user
---

# Tasks

`atm task` is the canonical lifecycle interface for durable ATM work. A task
has a stable task id (including an external Beads-shaped id), ordered
assignment attempts, and one of `assigned`, `active`, `blocked`, or a terminal
`closed` outcome. A task projection never stores a duplicate description or
message body: use its assignment message id with `atm read` for the rendered
instructions.

Assignment is template-first. Preview a template with `atm compose`, then use
the same source for the durable assignment:

```sh
atm task assign cipher t-42 --template task.xml.j2 --vars task-vars.json
atm task start t-42
atm task block t-42 --reason "waiting for API credentials"
atm task unblock t-42 --resolution "credentials received"
```

Plain text, `--file`, and `--stdin` are also supported message sources. Each
assignment mail requires acknowledgement and stays readable until acknowledged,
but acknowledgement is mail-only: it never starts a task. Only `atm task
start <task-id>` moves an assigned task to active.

An active assignee closes through a durable handoff to another same-host team
member. Closure and handoff mail are one atomic operation:

```sh
atm task complete t-42 --handoff team-lead --template completion.xml.j2 --vars result.json
atm task fail t-42 --handoff team-lead "validation failed: see the linked report"
atm task abort t-42 --handoff team-lead --reason cancelled "work withdrawn"
```

Cross-host task assignment and canonical handoff are rejected because Phase AZ
does not split a local task transaction across hosts. Reassigning or reopening
creates a new attempt; it never edits the previous assignment in place.

Inspect current work or terminal history with bounded queries:

```sh
atm task list
atm task list cipher
atm task list --as cipher --closed --limit 100 --json
atm task events t-42 --all --json
```

Open lists show active work first, then assigned tasks by priority and original
assignment time, then blocked tasks. `age` is elapsed time since the original
assignment. Closed history and event history are bounded to 200 rows unless
you select `--limit` (at most 10,000) or explicit `--all`.

When your runtime reports you idle, ATM may send one bounded reminder for the
top runnable task or one queued message. These lanes alternate fairly when
both are due. A reminder identifies the assignment message and task only; read
the assignment with `atm read` for its instructions. A reminder never starts,
acknowledges, or closes the task, and lifecycle-blocked or closed tasks receive
no normal reminder.

`atm send --task-id`, `atm send --task-complete`, `atm list --tasks`, and
`atm list --task-events` remain temporary compatibility adapters. Prefer the
canonical commands above; their migration warnings identify the replacement.

Return to the [ATM User Guide](./README.md).
