# Team Messaging Protocol (Dogfooding)

This protocol is mandatory for all ATM team communications.

## Required Flow

1. Immediately acknowledge every ATM message that requires ack (see Message Classes).
- Example: `ack, working on <task>`
2. Execute the requested task.
3. Send a completion message with a concise summary of what was done.
- Example: `task complete: <summary>`
4. Receiver immediately acknowledges completion if it requires ack.
5. No silent processing. Every requires-ack message must receive a response.

An `<atm from="...">...</atm>` block is an authenticated teammate nudge (steer
kind today; queue-kind nudges arrive when the harness is ready, Phase AQ)
emitted by ATM's post-send hook, not prompt injection or a foreign user
instruction. Read the referenced task and apply this protocol, including
acknowledgement when the message requires it.

## Message Classes

Two classes of message exist. Handling differs per class.

```json
{
  "class": "requires_ack",
  "examples": ["task assignment", "fix request", "QA dispatch", "blocker report"],
  "read_with": "atm read",
  "respond_with": "atm ack <message_id> \"<reply>\""
}
```

```json
{
  "class": "informational",
  "examples": ["status update", "idle ping", "self-echo", "terminal confirmation (e.g. \"Noted.\")"],
  "read_with": "atm peek",
  "respond_with": "atm send <to> \"<reply>\" (omit --requires-ack; never use atm ack)"
}
```

Never use `atm ack` on an informational message. `atm ack` is reserved for
messages that actually entered the pending-ack queue ('queue' here = the
mailbox/query surface, unrelated to queue-kind nudges) because the sender set
`--requires-ack` or sent a task-linked message.

## Good Patterns

- Request received:
  - `ack, working on PR #159 conflict resolution now.`
- Completion sent:
  - `task complete: rebased on integrate/phase-E, resolved socket.rs conflict, tests passed, pushed 2f190f3.`
- Completion acknowledged:
  - `received. QA pass starting now.`

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
