---
name: atm-hermes-ready
description: Verify the Hermes agents on a fixture are running and reachable over ATM before hermes-atm tests start: gateway processes alive, graft receivers registered with the daemon, and each agent answers a ping. Produces the standard ATM test report.
---

# atm-hermes-ready

One sentence triggers it: "run the atm-hermes-ready skill for <agent1@hermes agent2@hermes …>
and send the report to <agent@team>". The expected agent list comes from the request or, if
absent, from `atm members --team hermes`.

## Steps

1. **Roster.** `atm members --team hermes --json` lists every expected agent. Observable: missing
   names.
2. **Receivers registered.** `atm doctor --team hermes --json`, field `.graft_receivers.receivers[]`:
   one entry per expected agent with a `last_seen` newer than 10 minutes. Print only the `agent`
   and `last_seen` fields; never print or copy `endpoint` or `capability`. Observable: agents
   without a receiver entry, stale entries.
3. **Ping.** Send each expected agent one line, `--requires-ack`, asking for an ack with the reply
   "ready". Observable: message id per agent.
4. **Pong.** Within 120 s every agent's ack reply is in your inbox (`atm list --unread`, then read
   each). Observable: seconds per agent, or timeout. Any timeout is FAIL for that agent.

Never restart gateways or the daemon from this skill; report and stop.

## Report

Exactly one message to the requester, template `../atm-smoke/REPORT.md` (sibling skill directory), skill name
`atm-hermes-ready`, one step line per agent for steps 2–4.
