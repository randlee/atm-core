# `atm-core::config`

Owns configuration discovery, precedence rules, alias and bridge-hostname
resolution, and translation into validated core request inputs.

Also owns persisted config/team loading policy:
- deterministic compatibility defaults for documented schema drift
- classification of missing-document, record-level, and document-level failures
- recovery guidance and parser-context preservation for config errors
- refusal to guess identity or routing data during recovery

References:

- Product requirements: `docs/requirements.md` §3.3, §3.4, and §4
- `REQ-P-CONTRACT-001`
- `REQ-P-IDENTITY-001`
- `REQ-P-CONFIG-HEALTH-001`
- `REQ-CORE-CONFIG-001` for `[atm].team_members`, obsolete `[atm].identity`,
  and `post_send_hook` / `post_send_hook_senders` /
  `post_send_hook_recipients`
- `REQ-CORE-CONFIG-002` for `[atm].aliases` resolution and canonical address
  rewrite
- `REQ-CORE-CONFIG-003`
- `REQ-CORE-MAILBOX-001`
- Migration artifact: `docs/archive/file-migration-plan.md`
