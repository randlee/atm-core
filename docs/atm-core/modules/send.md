# `atm-core::send`

Owns send request validation, message construction, summary generation,
ack-required/task metadata handling, and atomic inbox append orchestration.

Also owns send-time resilience behavior that is not generic config parsing:
- missing-team-config fallback when the product contract explicitly allows it
- actionable sender warnings for degraded send behavior
- best-effort deduplicated repair notifications to `team-lead`

References:

- Product requirements: `docs/requirements.md` §6 and §12
- `REQ-P-SEND-001`
- `REQ-P-WORKFLOW-001`
- `REQ-P-CONFIG-HEALTH-001`
- `REQ-CORE-SEND-001`
- `REQ-CORE-MAILBOX-001`
- CLI surface: `docs/atm/commands/send.md`
