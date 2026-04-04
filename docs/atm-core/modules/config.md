# `atm-core::config`

Owns configuration discovery, precedence rules, alias and bridge-hostname
resolution, and translation into validated core request inputs.

Also owns persisted config/team loading policy:
- deterministic compatibility defaults for documented schema drift
- classification of record-level versus document-level parse failures
- recovery guidance and parser-context preservation for config errors

References:

- Product requirements: `docs/requirements.md` §3.3, §3.4, and §4
- `REQ-P-CONTRACT-001`
- `REQ-P-IDENTITY-001`
- `REQ-P-CONFIG-HEALTH-001`
- `REQ-CORE-CONFIG-003`
- `REQ-CORE-MAILBOX-001`
- Migration artifact: `docs/file-migration-plan.md`
