# AQ.1 Search-Projection Ledger Schema Approval

**Status:** pending product-owner approval

This is the required gate artifact for AQ.1. The AQ.1 implementation PR must
update this record before adding the migration. Approval is valid only when it
contains all fields below; an unchecked item blocks migration code.

- [ ] Product-owner approval reference: ATM message ID, issue/PR comment URL,
  or ADR amendment ID.
- [ ] Approved exact schema: ledger table name, primary/coalescing keys,
  source-identity columns, retry metadata, and indexes.
- [ ] Approved migration behavior: existing projections stay in place, ledger
  begins empty, and normal startup performs no full rebuild.
- [ ] Approved operational bounds: idle interval, batch bound, transaction
  deadline, retry limit/backoff, and stale threshold.
- [ ] Implementer and reviewer recorded in the AQ.1 implementation PR
  description under `AQ.1 schema approval` with a link to this artifact.

The implementation PR may begin test scaffolding that does not define or ship
the persistent schema, but it must not add the migration or enable the drain
until this artifact is complete.
