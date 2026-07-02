# Live Shared Inbox Validation

Superseded.

This exploratory design note assumed a forward `metadata.atm` expansion path.
Phase U abandoned that direction.

Current rule:
- ATM-owned runtime/query state must not depend on `metadata.atm`
- Claude-compatible JSON remains a boundary format only
- authoritative state lives in SQLite and approved ATM-owned config/store
  surfaces

Use instead:
- `docs/atm-message-schema.md`
- `docs/architecture.md`
- `docs/requirements.md`
- `docs/plans/phase-U/plan-phase-U.md`
