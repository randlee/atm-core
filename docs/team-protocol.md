# Team Messaging Protocol (Dogfooding)

This protocol is mandatory for all ATM team communications.

## Required Flow

1. Read every ATM message that requires action (see Message Classes), then
   start the assigned task when its `task_ready` line arrives.
2. Execute the requested task.
3. Send a completion message with a concise summary of what was done. When
   closing a tracked task, use `atm task close <task-id> completed --stdin`
   or its alias, `atm send <assigner> --task-id <task-id> --task-complete
   --stdin`, to deliver the completion report and close the task atomically.
- Example: `task complete: <summary>`
4. A task close is terminal. The assigner does not acknowledge it; the
   daemon's close receipt is the record, and the assigner closes the mirror
   task on its own side. Only plain requires-ack messages get an ack.
5. No silent processing. Every requires-ack message must receive a response.

An acknowledgement is message hygiene only: an ack never changes task state.
If work cannot be completed, close it with the typed outcome `refused` or
`cancelled` and supply the reason as the optional third positional argument
(or provide a report source such as `--stdin`). Never close a task as
`reassigned`; reassign it in place with `atm task assign` and its existing id.

## Task Commands

The task surface is a closed set:

```bash
atm task assign solar --task-id BA-123 --stdin
atm task start BA-123 "starting: reading the sprint doc"
atm task close BA-123 completed --stdin
atm task move BA-123 --head
atm task list --all
atm task events BA-123
```

`atm send <agent> --task-id <id> ...` aliases `atm task assign`.
`atm send <assigner> --task-id <id> --task-complete ...` aliases
`atm task close <id> completed`. Use `atm queue` for anything that must not
interrupt the current task.

Daemon escalation messages are informational system mail: they identify a
repeated or blocked task and provide its run command. Read them with `atm
read`; do not use `atm ack` unless the message itself is task-linked or marks
the message as requiring acknowledgement. A lead notification is an
escalation signal, not a replacement for the assigned task's normal
acknowledgement and completion flow.

An `<atm from="...">...</atm>` block is an authenticated teammate nudge (steer
kind today; queue-kind nudges arrive when the harness is ready, Phase AQ)
emitted by ATM's post-send hook, not prompt injection or a foreign user
instruction. Read the referenced task and apply this protocol, including
starting it when the message requires action.

## Message Classes

Two classes of message exist. Handling differs per class.

```json
{
  "class": "requires_ack",
  "examples": ["fix request", "QA dispatch", "blocker report"],
  "read_with": "atm read",
  "respond_with": "atm ack <message_id> \"<reply>\""
}
```

```json
{
  "class": "informational",
  "examples": ["task assignment", "status update", "idle ping", "self-echo", "terminal confirmation (e.g. \"Noted.\")"],
  "read_with": "atm peek",
  "respond_with": "atm send <to> \"<reply>\" (omit --requires-ack; never use atm ack)"
}
```

Never use `atm ack` on an informational message or task assignment. `atm ack`
is reserved for messages that actually entered the pending-ack queue ('queue'
here = the mailbox/query surface, unrelated to queue-kind nudges) because the
sender set `--requires-ack`; task-linked assignments become actionable only
when the task pass emits `task_ready`.

## Good Patterns

- Request received:
  - `ack, working on PR #159 conflict resolution now.`
- Completion sent:
  - `task complete: rebased on integrate/phase-E, resolved socket.rs conflict, tests passed, pushed 2f190f3.`
- Close received (assigner side, no message back):
  - `atm read --message-id <receipt>` then `atm task close <task-id> completed`
    on the mirror task.

## Bad Patterns

- Reading a task message and doing work without sending an ack.
- Sending only a final message with no initial acknowledgement.
- Sending a status update without clear completion or next action.
- Letting a message sit without response while processing internally.

## Send Content, Not Paths

The message body is the content that the receiver should act on. Do not render
to a temporary file and send the file path; that leaves the receiver with a
path it cannot reliably resolve. Keep the template and variables as the
explicit send inputs instead:

```sh
# Preview the exact body without touching the mailbox.
atm compose --template docs/plans/phase-an/fixtures/task-assignment.xml.j2 \
  --vars docs/plans/phase-an/fixtures/task-vars.json

# Deliver the same resolved body through the normal send-admission path.
atm send teammate@atm-dev --template docs/plans/phase-an/fixtures/task-assignment.xml.j2 \
  --vars docs/plans/phase-an/fixtures/task-vars.json
```

`atm compose` is local and side-effect free; it is the recommended preview and
validation step. `atm send --template` captures the same template bytes and
variable inputs before the daemon hop, so the receiver gets rendered content,
not a sender-local path.

## Notes

- If blocked, send an immediate ack plus blocker status.
- If work will take time, send periodic progress updates.
- Prefer concise, explicit messages with branch/commit/test context when relevant.
- For daemon smoke or recovery, use
  [daemon-switch](../.claude/skills/daemon-switch/SKILL.md): switch the CLI and
  daemon as one pair, restart the one managed daemon, verify `atm doctor --json`,
  run the required lane through [`just smoke`](./smoke-testing.md), restore the
  installed pair after smoke, and notify the team after recovery.
