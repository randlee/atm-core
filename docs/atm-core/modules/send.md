# `atm-core::send`

Owns send request validation, message construction, summary generation,
ack-required/task metadata handling, and atomic inbox append orchestration.

Also owns send-time resilience behavior that is not generic config parsing:
- missing-team-config fallback when the product contract explicitly allows it
- actionable sender warnings for degraded send behavior
- best-effort deduplicated repair notifications to `team-lead`
- post-send-hook trigger evaluation, payload construction, and diagnostics

Accepted limitations in this module:
- missing-config repair notifications are best-effort and effectively at-most-once across crash windows because dedup state is recorded before the team-lead inbox append
- send-alert stale-lock eviction uses PID-only liveness checks, so PID reuse can conservatively preserve a stale lock until manual cleanup or timeout

`SendOutcome.warnings` is part of the stable send API contract:
- empty during normal sends
- populated only when send succeeds in a degraded but permitted mode
- contains actionable human-readable warning text for the caller to surface

Post-send-hook contract owned by this module:
- reject retired `post_send_hook_members` config with migration guidance
- evaluate sender and recipient hook triggers with `*` wildcard support
- run the hook at most once per successful send even when both axes match
- populate `ATM_POST_SEND` with trigger booleans so one script can branch on
  sender- versus recipient-triggered execution
- optionally parse one structured stdout result from the hook for observability
  without making hook output mandatory
- preserve actionable warnings and structured diagnostics when a hook is
  configured but skipped or when execution fails

References:

- Product requirements: `docs/requirements.md` §6 and §14
- `REQ-P-SEND-001`
- `REQ-P-WORKFLOW-001`
- `REQ-P-CONFIG-HEALTH-001`
- `REQ-CORE-CONFIG-001` for runtime identity precedence and obsolete
  `[atm].identity` handling
- `REQ-CORE-CONFIG-002` for alias rewrite and canonical target resolution
- `REQ-CORE-SEND-001` for missing-config fallback and repair notification
- `REQ-CORE-SEND-002` for `metadata.atm.fromIdentity` placement when
  cross-team alias projection is used
- `REQ-CORE-SEND-003` for send-path message construction and append-boundary
  behavior
- `REQ-CORE-MAILBOX-001`
- CLI surface: `docs/atm/commands/send.md`
