---
title: Identity And Team
audience: end-user
reviewed_for_release: 1.4.2
---

# Identity And Team

ATM commands depend on a resolved team and, for caller-owned workflows, a
resolved identity.

## Core Rule

If a command needs caller identity, ATM must know who you are. That resolution
comes from the supported CLI and environment surfaces for the active command.

## Resolution Model

The accepted operator model is:

- inspection-only commands may inspect another mailbox explicitly
- mutating commands require the ambient caller and cannot impersonate another
  base agent
- command-line values override environment values when that command supports
  the override

Practical examples:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm

# Real caller mutates their own workflow state.
atm send quality-mgr@atm-dev "review smoke lane"
atm read --team atm-dev
atm ack 01KRFK5QTF2R6NRS3Q0F8Z9K0S "received"

# Inspection-only commands may target another mailbox explicitly.
atm list quality-mgr@atm-dev --team atm-dev --as quality-mgr
atm peek quality-mgr@atm-dev --team atm-dev --as quality-mgr
```

## Safety Model

Mailbox inspection and mailbox mutation are different surfaces:

- inspection commands observe queue state
- mutating commands act as the real caller. Where a mutating command accepts
  `--as`, its base agent must match `ATM_IDENTITY`; it may select the caller's
  chat identity but cannot impersonate another agent.

Inspection-only surfaces:

- `atm list`
- `atm peek`

Mutating surfaces:

- `atm send`
- `atm read`
- `atm ack`
- `atm clear`

## Team And Identity Guidance

- use `ATM_TEAM` when you normally operate in one ATM team
- use `ATM_IDENTITY` when your caller identity should be resolved from the
  environment
- use `ATM_CHAT_ID` to select the optional ambient chat identity. Chat
  precedence is `--as agent:chat`, `--chat-id`, `ATM_CHAT_ID`, qualified
  `ATM_IDENTITY`, then no chat ID. An unqualified `--as agent` explicitly
  selects no chat ID.
- use `--team <team>` when the command supports an explicit team override
- use `--as <agent>` to inspect that agent's mailbox on inspection-only
  commands. On a mutating command that accepts `--as`, use only the same base
  agent as `ATM_IDENTITY`.

Do not assume that runtime state under `~/.atm/` changes your caller identity.
Caller identity is resolved by ATM command inputs and environment rules, not by
editing local state on disk.

Additional machine-readable examples live in
[examples/identity/](./examples/identity/).

For queue workflows, continue to [Mailbox Workflows](./mailbox-workflows.md).

Return to the [ATM User Guide](./README.md).
