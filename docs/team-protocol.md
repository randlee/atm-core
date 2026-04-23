# Team Messaging Protocol (Dogfooding)

This protocol is mandatory for all ATM team communications.

## Required Flow

1. Immediately acknowledge every ATM message received.
- Example: `ack, working on <task>`
2. Execute the requested task.
3. Send a completion message with a concise summary of what was done.
- Example: `task complete: <summary>`
4. Receiver immediately acknowledges completion.
5. No silent processing. Every message must receive a response.

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

## Notes

- If blocked, send an immediate ack plus blocker status.
- If work will take time, send periodic progress updates.
- Prefer concise, explicit messages with branch/commit/test context when relevant.
