# Dedup Metadata Schema

Superseded.

This design note proposed `metadata.atm` as the forward ATM-owned machine-state
surface. Phase U abandoned that schema direction.

Current rule:
- no active ATM-owned machine-state namespace exists under `metadata.atm`
- ATM must not introduce new runtime/query dependence on `metadata.atm`
- one logical message identity is retained; duplicated ATM-owned message-id
  layers are removal targets

Use instead:
- `docs/atm-message-schema.md`
- `docs/architecture.md`
- `docs/requirements.md`
- `docs/plans/phase-U/sprint-U1.md`
- `docs/plans/phase-U/sprint-U2.md`
