---
title: Troubleshooting
audience: end-user
reviewed_for_release: 1.3.0
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

## Related Documents

- [Identity And Team](./identity-and-team.md)
- [Mailbox Workflows](./mailbox-workflows.md)
- [Doctor And Log](./doctor-and-log.md)
- [Hooks](./hooks.md)
- [Nudge Templates](./nudge-templates.md)

Return to the [ATM User Guide](./README.md).
