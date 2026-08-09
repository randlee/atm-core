---
name: hermes-gateway
description: "List Hermes agent gateway status or reset one or more named Hermes agent gateways. Use when asked to check, list, restart, or reset a Hermes gateway. Triggers: hermes gateway status, list hermes agents, reset skillrx gateway, restart Hermes gateway."
---

# Hermes Gateway

List gateway state, inspect one Hermes agent, or reset one or more explicitly
named agents. Invoke the vendored utility at
`.claude/skills/hermes-gateway/scripts/hermes_gateway`; do not assume a bare
`hermes_gateway` command is on `PATH`.

The utility rejects unknown profiles and accepts only explicit profile names
for reset. It does not provide a wildcard or implicit fleet reset.

## Invocation

```
/hermes-gateway --list                         # list every agent gateway state
/hermes-gateway <agent_name>                    # inspect one named agent
/hermes-gateway <agent_name> [<agent> ...] --reset # reset named agents
```

- `--list` is status-only and makes no changes.
- A named-agent status command makes no changes.
- `--reset` follows one or more explicit names; it first prints each target's
  state, then resets that target. It does not accept `all`.

## Status Procedure

For one named agent, run:

```
.claude/skills/hermes-gateway/scripts/hermes_gateway <agent_name>
```

For an inventory, run `.claude/skills/hermes-gateway/scripts/hermes_gateway
--list`. It reports every configured profile's LaunchAgent state and PID where
available.

## Reset Procedure

1. Confirm explicit target names. Do not reset the gateway currently carrying
   your own session without the user's confirmation.
2. Run `.claude/skills/hermes-gateway/scripts/hermes_gateway <agent>
   [<agent> ...] --reset`. The utility reports status before each action,
   uses `launchctl kickstart -k` for loaded services, and bootstraps absent
   services.
3. Re-run the named-agent status command after the reset. Report any target
   that remains stopped or dead; do not retry indefinitely.

## Non-Goal

Does not reset an unbounded set of agents; every reset target is named.
