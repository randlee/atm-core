---
title: Tasks
audience: end-user
---

# Tasks

ATM task mail tracks one durable task for an assignee through three states:
`assigned`, `active`, and `complete`. Sending mail with `--task-id` creates or
updates an assigned task and requires acknowledgement. A successful
acknowledgement makes it active; an assignee cannot activate a second task
while another task is active.

The assignee or the assigner closes an open task by sending its completion to
the assignee:

```sh
atm send cipher --task-complete t-42 --stdin
```

Completion is valid from either `assigned` or `active`. Completing an assigned
task also acknowledges its assignment, so the assignment is not left in the
pending-ack queue.

Inspect the durable task ledger with either of these surfaces:

```sh
atm list --tasks
atm list --tasks --member cipher --json
atm list --task-events t-42
atm list --task-events t-42 --member cipher --json
```

`--tasks --json` returns an array of task rows. `--task-events <id> --json`
returns that task's append-only events in sequence order. An unknown task id
prints the event header only (or `[]` as JSON) and succeeds. See ADR-061 for
the task tables and audit/replay contract.

Return to the [ATM User Guide](./README.md).
