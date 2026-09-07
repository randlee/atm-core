---
name: atm-nudge-roundtrip
description: The hermes-atm integration test: a tester agent sends an ack-required message to a Hermes agent, the Hermes agent is nudged, reads that exact message id with its native atm_read, acks natively, and the tester sees the ack. Both sides run the same skill. Produces the standard ATM test report.
---

# atm-nudge-roundtrip

Two roles, one skill. The request says which role you are.

- Tester: "run the atm-nudge-roundtrip skill as tester against <agent@hermes>, report to <agent@team>".
- Hermes side: "run the atm-nudge-roundtrip skill as responder, report to <agent@team>". The Hermes
  side normally needs no instruction: the nudge itself starts the responder steps.

## Tester steps

1. **Send.** `atm send <hermes agent> --requires-ack --stdin <<'EOF'` with the single line
   `atm-nudge-roundtrip <run-id>: ack this message with reply "roundtrip <run-id>"` where run-id is
   the current unix time. Observable: message id, exit code.
2. **Ack arrives.** Poll `atm list --unread --json` every 10 s for up to 180 s until a message from
   that agent containing `roundtrip <run-id>` appears; read it. Observable: seconds, or timeout.
3. **State.** `atm list --pending-ack --json`: the id from step 1 is no longer pending.
   Observable: present yes/no.

## Responder steps (Hermes agent)

1. **Nudge received.** You received an `<atm …>` block naming a message id. Observable: the id.
2. **Read by id.** `atm_read(message_id=<id>)`. Observable: `count` (must be 1); FAIL with the code
   if 0 or error.
3. **Ack natively.** `atm_ack(message_id=<id>, reply="roundtrip <run-id>")`. Observable: exit/ error
   code.
4. **Self round trip after idle.** Wait 5 s, `atm_send` one line to yourself, `atm_read` it by id.
   Observable: `count`, error code if any.

## Report

Exactly one message to the requester, template in `.claude/skills/atm-smoke/REPORT.md`, skill name
`atm-nudge-roundtrip`, first step line states the role.
