# ADR Index

This index lists the repository-wide ADRs in `docs/adr/` and the accepted
crate-local ADR records that remain embedded in crate architecture documents.

## Repository ADRs

- [ADR-001 — Sealed Trait Pattern](./ADR-001-sealed-trait-pattern.md)
- [ADR-002 — Host-Wide ATM Daemon Singleton](./ADR-002-host-wide-daemon-singleton.md)
- [ADR-003 — Test Fidelity And Daemon Isolation](./ADR-003-test-fidelity-and-daemon-isolation.md)
- [ADR-005 — Host-Scoped SQLite State Root](./ADR-005-host-scoped-sqlite-state-root.md)
- [ADR-006 — Bounded SIGHUP Reload Delivery In R.18](./ADR-006-sighup-reload-deferral.md)
- [ADR-007 — Supported Platform Feature Parity](./ADR-007-supported-platform-parity.md)
- [ADR-008 — No-Flaky-Test Policy And Mechanical Enforcement](./ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md)
- [ADR-009 — Bounded Queue Query Surface](./ADR-009-bounded-queue-query-surface.md)

## Embedded Crate-Local ADR Records

These accepted ADR records are intentionally embedded in the crate architecture
documents for now and must be reviewed as part of those crate docs:

- `ADR-ATM-DAEMON-001` — `docs/atm-daemon/architecture.md`
  - daemon is the current runtime composition root
- `ADR-ATM-CORE-001` — `docs/atm-core/architecture.md`
  - shared ATM protocol lives in `atm-core`
- `ADR-ATM-001` — `docs/atm/architecture.md`
  - CLI uses shared protocol and client transport only

Until they are extracted into standalone files, this index is the canonical
place that names and locates the crate-local ADR records.
