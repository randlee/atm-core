# `atm-core::send`

Owns send request validation, message construction, summary generation,
ack-required/task metadata handling, and atomic inbox append orchestration.

Also owns send-time resilience behavior that is not generic config parsing:
- missing-team-config fallback when the product contract explicitly allows it
- actionable sender warnings for degraded send behavior
- best-effort deduplicated repair notifications to `team-lead`

`SendOutcome.warnings` is part of the stable send API contract:
- empty during normal sends
- populated only when send succeeds in a degraded but permitted mode
- contains actionable human-readable warning text for the caller to surface

References:

- Product requirements: `docs/requirements.md` §6 and §14
- `REQ-P-SEND-001`
- `REQ-P-WORKFLOW-001`
- `REQ-P-CONFIG-HEALTH-001`
- `REQ-CORE-CONFIG-001` for runtime identity precedence and obsolete
  `[atm].identity` handling
- `REQ-CORE-CONFIG-002` for alias rewrite and canonical target resolution
- `REQ-CORE-SEND-001`
- `REQ-CORE-SEND-002` for `metadata.atm.fromIdentity` placement when
  cross-team alias projection is used
- `REQ-CORE-MAILBOX-001`
- CLI surface: `docs/atm/commands/send.md`
