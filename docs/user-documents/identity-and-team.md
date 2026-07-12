---
title: Identity And Team
audience: end-user
reviewed_for_release: 1.3.0
---

# Identity And Team

ATM commands depend on a resolved team and, for caller-owned workflows, a
resolved identity.

## Core Rule

If a command needs caller identity, ATM must know who you are. That resolution
comes from the supported CLI and environment surfaces for the active command.

## Safety Model

Mailbox inspection and mailbox mutation are different surfaces:

- inspection commands observe queue state
- mutating commands act as the real caller

Do not assume that runtime state under `~/.atm/` changes your caller identity.
Caller identity is resolved by ATM command inputs and environment rules, not by
editing local state on disk.

For queue workflows, continue to [Mailbox Workflows](./mailbox-workflows.md).

Return to the [ATM User Guide](./README.md).
