---
title: Hooks
audience: end-user
reviewed_for_release: 1.4.2
---

# Hooks

ATM can integrate with supported hook surfaces for startup, stop, idle, and
message-related workflows.

This document covers the operator-facing hook model only. It does not describe
developer implementation internals.

## Hook Scope

Hook configuration belongs to the ATM-enabled repo surface for the active team
and identity.

Repo-local ATM hook configuration lives in `.atm.toml`.

Supported operator-facing hook/config surfaces include:

- `[[atm.post_send_hooks]]` for recipient-scoped external post-send commands
- `[startup.<identity>]` for startup text injection for the named identity
- `[atm.idle_notify]` and `[atm.idle_notify.agent.<identity>]` for idle
  notification routing and per-agent thresholds

Example:

```toml
[atm]
default_team = "atm-dev"

[atm.idle_notify]
recipient = "team-lead"

[atm.idle_notify.agent.arch-ctm]
seconds = 60

[[atm.post_send_hooks]]
recipient = "quality-mgr"
command = ["post-send-notify.sh"]

[startup.arch-ctm]
all = [
  "Use native atm send or atm ack for messages to team-lead or quality-mgr",
  "Use native atm read to read messages from team-members"
]
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

ATM-aware hook lookup does not follow the invoking shell `cwd`. When
`ATM_IDENTITY` and `ATM_TEAM` resolve an ATM-enabled caller, ATM loads the
authoritative roster record for that sender and resolves repo-local hook paths
from that sender's stored `home_dir`. In practice, that means the same
repo-local hook configuration still applies when an agent runs `atm send` from
another folder, because ATM uses the sender's roster-backed ATM home instead of
guessing from the current shell location.

## Startup And Idle Behavior

Startup and idle configuration are distinct from post-send hooks:

- startup entries define identity-specific startup text
- idle notification config controls who receives an idle notification and when
- neither of those surfaces changes durable mailbox truth

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
