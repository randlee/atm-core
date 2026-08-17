---
title: Mailbox Workflows
audience: end-user
reviewed_for_release: 1.4.2
---

# Mailbox Workflows

ATM mailbox workflows split queue inspection from message mutation.

## Queue Inspection

Use inspection-oriented commands when you need to see what is waiting without
changing message state.

Inspection-only surfaces:

- `atm list`
- `atm peek`

Typical inspection flow:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm

atm list quality-mgr@atm-dev --team atm-dev --as quality-mgr --json
atm peek quality-mgr@atm-dev --team atm-dev --as quality-mgr --json
```

## Message Handling

Use message-handling commands when you intend to read, acknowledge, clear, or
reply through the supported ATM workflow.

Caller-scoped mutating surfaces:

- `atm send`
- `atm read`
- `atm ack`
- `atm clear`

Typical caller-scoped flow:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm

atm send quality-mgr@atm-dev "review the current branch"
atm read --team atm-dev
atm ack 01KRFK5QTF2R6NRS3Q0F8Z9K0S "received"
atm clear --team atm-dev --dry-run
```

For template-backed delivery, first use `atm compose --template <path>` to
preview the exact local rendering, then use `atm send <recipient> --template
<path>` with the same variables. Composition is local and does not mutate a
mailbox.

## Workflow Guidance

- inspect before mutating when you need to confirm queue state
- read the actual message before acting on it
- use the supported acknowledge path when a task or sender requires it
- use `atm clear` only for the resolved caller's mailbox state
- do not use inspection examples as a substitute for caller-scoped mutation
- a mutating command's optional `--as` value may only name the same base agent
  as `ATM_IDENTITY`; it can select that caller's chat identity, never another
  agent's mailbox state

## Clear Guidance

`atm clear` is the retained cleanup surface for read or acknowledged mailbox
state. It acts as the resolved caller only.

Examples:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm

atm clear --team atm-dev --dry-run
atm clear --team atm-dev --older-than 7d
atm clear --team atm-dev --idle-only
```

Additional runnable examples live in [examples/mailbox/](./examples/mailbox/).

For caller-resolution guidance, see [Identity And Team](./identity-and-team.md).
For recovery and queue diagnostics, see [Troubleshooting](./troubleshooting.md).

Return to the [ATM User Guide](./README.md).
