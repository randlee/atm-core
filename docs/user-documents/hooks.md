---
title: Hooks
audience: end-user
reviewed_for_release: 1.6.0
---

# Hooks

ATM can run supported post-send hook commands for message-related workflows.

This document covers the operator-facing hook model only. It does not describe
developer implementation internals.

## Hook Scope

Hook configuration belongs to the ATM-enabled repo surface for the active team
and identity.

Repo-local ATM hook configuration lives in `.atm.toml`.

The supported operator-facing hook/config surface is
`[[atm.post_send_hooks]]` for recipient-scoped external post-send commands.

Example:

```toml
[atm]
default_team = "atm-dev"

[[atm.post_send_hooks]]
recipient = "quality-mgr"
command = ["post-send-notify.sh"]
```

## Post-Send Hook Rules

`[[atm.post_send_hooks]]` is the supported full override path for post-send
behavior.

Important rules:

- `recipient` is one concrete member name or `*`
- multiple matching rules may run in config order
- path-like `command[0]` values resolve relative to the declaring `.atm.toml`
- bare executable names use normal `PATH` resolution
- post-send hooks are best-effort side effects and do not redefine whether ATM
  durably accepted a message
- if no matching external post-send hook rule exists, ATM falls back to the
  shipped built-in nudge path

The hook payload arrives in `ATM_POST_SEND` as ATM-owned JSON. That payload
includes the sender, recipient, team, `message_id`, description, task id,
ack-related flags, and other supported post-send fields.

## Installed Docs vs Runtime State

Hook docs are installed under `share/doc/atm/`. Runtime state stays under
`~/.atm/`. Do not treat runtime state as the authoritative source for installed
long-form help content.

Example files live in [examples/hooks/](./examples/hooks/).

## Related Topics

- nudge payload shape and built-in overrides: [Nudge Templates](./nudge-templates.md)
- identity-sensitive behavior: [Identity And Team](./identity-and-team.md)
- recovery steps when a hook path fails: [Troubleshooting](./troubleshooting.md)

Return to the [ATM User Guide](./README.md).
