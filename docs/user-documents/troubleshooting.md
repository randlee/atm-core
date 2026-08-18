---
title: Troubleshooting
audience: end-user
reviewed_for_release: 1.4.3
---

# Troubleshooting

Use this guide when ATM behavior does not match the expected command workflow.

## First Checks

- confirm you are in the correct repo and team context
- confirm your caller identity is resolved for the command you are running
- run `atm doctor`
- inspect ATM logs through the supported log surface

## Common Problem Areas

- wrong team or identity context
- queue state confusion between inspection and mutating commands
- hook or nudge configuration drift
- install-tree expectations mixed with runtime-state expectations

## Identity Or Team Not Resolved

Symptoms:

- ATM refuses a mutating command because caller identity is unknown
- ATM cannot resolve the active team for the command

Supported recovery:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm
atm doctor --team atm-dev
```

If the command supports `--team`, you may supply it explicitly. Do not try to
repair identity problems by editing local database files.

## Daemon Startup Or Connect Failure

Symptoms:

- ATM cannot reach the local daemon path
- the command reports daemon startup/connect trouble

The managed daemon uses host-scoped state. Changing `ATM_HOME` does not select
another daemon, endpoint, database, or retained-log root.

Supported recovery:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm

atm doctor --team atm-dev --json
atm log snapshot --limit 50
atm log filter --level error
```

Use doctor and retained logs first. Those are the supported operator-facing
surfaces for daemon/runtime recovery.

## Post-Send Warning Surface

Symptoms:

- send succeeds but ATM reports a warning after delivery
- a post-send notification does not behave as expected

Supported recovery:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm

ATM_LOG=debug atm send quality-mgr@atm-dev "review smoke lane" --stderr-logs
atm log filter --level warn --match command=send
```

Treat warning output as an operator-visible event. Do not assume silent success
when a post-send path reports a warning.

## Nudge Delivery Misconfiguration

Symptoms:

- a message lands but the expected nudge does not arrive
- the wrong nudge target receives a notification

Supported recovery:

- confirm the current repo/team context
- re-check the supported hook and nudge-template configuration
- use ATM logs to verify the post-send path

For the configuration model, see [Hooks](./hooks.md) and
[Nudge Templates](./nudge-templates.md).

Additional runnable examples live in
[examples/troubleshooting/](./examples/troubleshooting/).

## Related Documents

- [Identity And Team](./identity-and-team.md)
- [Mailbox Workflows](./mailbox-workflows.md)
- [Doctor And Log](./doctor-and-log.md)
- [Hooks](./hooks.md)
- [Nudge Templates](./nudge-templates.md)

Return to the [ATM User Guide](./README.md).
