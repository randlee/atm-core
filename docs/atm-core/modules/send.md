# `atm-core::send`

Owns send request validation, message construction, summary generation,
ack-required/task metadata handling, and atomic inbox append orchestration.

References:

- Product requirements: `docs/requirements.md` §6 and §12
- `REQ-P-SEND-001`
- `REQ-P-POST-SEND-001`
- `REQ-P-WORKFLOW-001`
- `REQ-CORE-MAILBOX-001`
- `REQ-POST-SEND-002`
- `REQ-POST-SEND-003`
- `REQ-POST-SEND-004`
- CLI surface: `docs/atm/commands/send.md`
