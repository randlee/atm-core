---
name: atm-smoke
description: The ATM messaging smoke test every agent runs the same way on every fixture: send, list, read by message id, peek, ack, and a stale-connection probe. Uses native ATM tools when the agent has them (Hermes atm_send/atm_read/atm_list/atm_ack), otherwise the atm CLI. Produces the standard ATM test report.
---

# atm-smoke

One sentence triggers it: "run the atm-smoke skill against <partner agent@team> and send the report
to <agent@team>". Run `atm-setup-environment` first on a fresh fixture.

Tool rule: use the native tool when you have it, otherwise the CLI form. Report which you used.

| step | native (Hermes) | CLI |
|---|---|---|
| send | `atm_send(to, body)` | `atm send <to> --stdin <<'EOF' … EOF` |
| list | `atm_list()` | `atm list --unread --json` |
| read by id | `atm_read(message_id=<id>)` | `atm read --message-id <id> --json` |
| peek | `atm_read(message_id=<id>, peek=True)` | `atm peek --message-id <id> --json` |
| ack | `atm_ack(message_id, reply)` | `atm ack <id> "<reply>"` |

## Steps

Partner = the agent named in the request (default: yourself).

1. **Send to self.** One-line body. Observable: returned message id; FAIL on any error code
   (`MAY_HAVE_EXECUTED` is a FAIL with that code).
2. **List shows it.** List unread; the id from step 1 is present. Observable: count, present yes/no.
3. **Read by id.** Read the id from step 1. Observable: `count` (must be 1) and, where reported,
   `mutation_applied` (must be true).
4. **Read marked it.** List unread again; the id is gone. Observable: present yes/no.
5. **Peek does not mutate.** Send a second one-line message to self, peek it by id, list unread:
   it must still be present. Then read it normally. Observable: present yes/no after peek.
6. **Stale-connection probe.** Wait 5 s doing nothing, then list. Observable: exit/ error code.
   FAIL if the call reports `MAY_HAVE_EXECUTED`, `RequestWrite`, or a connection error.
7. **Send to partner with ack required.** CLI: `atm send <partner> --requires-ack --stdin …`;
   native: `atm_send(to, body, requires_ack=True)`. Observable: message id. Skip with SKIP if the
   partner is yourself.
8. **Partner acks.** Within 120 s the partner's ack reply arrives in your inbox (list, then read it).
   Observable: seconds until the ack appeared, or timeout.

## Report

Exactly one message to the requester, template in `REPORT.md` next to this file, skill name
`atm-smoke`, `tools:` set to what you used.
