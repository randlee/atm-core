---
title: Mailbox Workflows
audience: end-user
reviewed_for_release: 1.3.0
---

# Mailbox Workflows

ATM mailbox workflows split queue inspection from message mutation.

## Queue Inspection

Use inspection-oriented commands when you need to see what is waiting without
changing message state.

## Message Handling

Use message-handling commands when you intend to read, acknowledge, clear, or
reply through the supported ATM workflow.

## Workflow Guidance

- inspect before mutating when you need to confirm queue state
- read the actual message before acting on it
- use the supported acknowledge path when a task or sender requires it

For caller-resolution guidance, see [Identity And Team](./identity-and-team.md).
For recovery and queue diagnostics, see [Troubleshooting](./troubleshooting.md).

Return to the [ATM User Guide](./README.md).
