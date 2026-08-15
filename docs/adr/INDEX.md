---
title: ADR Index
---

# ADR Index

This index lists the repository-wide ADRs in `docs/adr/` and the accepted
crate-local ADR records that remain embedded in crate architecture documents.

## Repository ADRs

- [ADR-001 — Sealed Trait Pattern](./ADR-001-sealed-trait-pattern.md)
- [ADR-002 — Host-Wide ATM Daemon Singleton (superseded by ADR-026)](./ADR-002-host-wide-daemon-singleton.md)
- [ADR-003 — Test Fidelity And Daemon Isolation](./ADR-003-test-fidelity-and-daemon-isolation.md)
- `ADR-004` — number reserved / withdrawn
- [ADR-005 — Host-Scoped SQLite State Root (superseded by ADR-026)](./ADR-005-host-scoped-sqlite-state-root.md)
- [ADR-006 — Bounded SIGHUP Reload Delivery In R.18](./ADR-006-sighup-reload-deferral.md)
- [ADR-007 — Supported Platform Feature Parity](./ADR-007-supported-platform-parity.md)
- [ADR-008 — No-Flaky-Test Policy And Mechanical Enforcement](./ADR-008-no-flaky-test-policy-and-mechanical-enforcement.md)
- [ADR-009 — Bounded Queue Query Surface](./ADR-009-bounded-queue-query-surface.md)
- [ADR-010 — Claude JSONL Compatibility Envelope](./ADR-010-claude-jsonl-compatibility-envelope.md)
- [ADR-011 — Host-Scoped Retained Log Root](./ADR-011-host-scoped-retained-log-root.md)
- [ADR-012 — One Message Identity](./ADR-012-one-message-identity.md)
- [ADR-013 — Unified Delivery Plan And State-Machine-Owned Path Decisions](./ADR-013-unified-delivery-plan-and-state-machine-ownership.md)
- [ADR-014 — Runtime Health Projection And Liveness Signal Ownership](./ADR-014-runtime-health-projection-and-liveness-signal-ownership.md)
- [ADR-015 — Daemon Runtime Snapshot Publication And Worker Ownership](./ADR-015-daemon-runtime-snapshot-and-worker-ownership.md)
- [ADR-016 — Claude Config Ingress And Roster Projection Ownership](./ADR-016-claude-config-ingress-and-roster-projection-ownership.md)
- [ADR-017 — Claude Inbox Fail-Soft Read Policy](./ADR-017-claude-inbox-fail-soft-read-policy.md)
- [ADR-018 — Storage Contract Reset And Backend Interchangeability](./ADR-018-storage-contract-reset-and-backend-interchangeability.md)
- [ADR-019 — Direct Post-Send Emission And Claude Backend Retirement](./ADR-019-direct-post-send-and-claude-json-retirement.md)
- [ADR-020 — RULE-001 Observability Adapter Exception](./ADR-020-rule001-observability-adapter-exception.md)
- [ADR-021 — NudgeTemplateOverrideStore Dependent Widening (superseded by ADR-024)](./ADR-021-nudge-template-override-store-dependent-widening.md)
- [ADR-022 — Durable Ack Intent](./ADR-022-durable-ack-intent.md)
- [ADR-023 — Owner-Only Message Mutation](./ADR-023-owner-only-message-mutation.md)
- [ADR-024 — NudgeTemplateOverrideStore Storage Ownership Relocation](./ADR-024-nudge-template-override-storage-ownership-relocation.md)
- [ADR-025 — Installed User Documentation Surface](./ADR-025-installed-user-documentation-surface.md)
- [ADR-026 — Host Singleton And Durable State Root](./ADR-026-host-singleton-and-durable-state-root.md)
- [ADR-027 — Client/Daemon Schema Compatibility](./ADR-027-client-daemon-version-compatibility.md)
- [ADR-028 — Cross-Host Interface Control Plane (superseded by ADR-034)](./ADR-028-cross-host-interface-control-plane.md)
- [ADR-029 — Cross-Host Host Authorization (superseded by ADR-034)](./ADR-029-cross-host-host-authorization.md)
- [ADR-030 — Cross-Host Transport Security Sequencing (superseded by ADR-034)](./ADR-030-cross-host-transport-security-phase.md)
- [ADR-031 — Remote-Target Contract And Cross-Host Dispatch Boundary (superseded by ADR-034/035)](./ADR-031-remote-target-contract-and-cross-host-dispatch.md)
- [ADR-032 — Unified Error Contract](./ADR-032-unified-error-contract.md)
- [ADR-033 — HTTP Endpoint Contract](./ADR-033-http-endpoint-contract.md)
- [ADR-034 — Minimal Cross-Host HTTPS Transport (superseded by ADR-047)](./ADR-034-minimal-cross-host-https-transport.md)
- [ADR-035 — Canonical Write Ingress And Host Routing (active ingress; transport wording superseded by ADR-047)](./ADR-035-canonical-write-ingress-and-host-routing.md)
- [ADR-036 — Storage Boundary And Composition Topology](./ADR-036-storage-boundary-and-composition-topology.md)
- [ADR-037 — Chat Address Identity](./ADR-037-chat-address-identity.md)
- [ADR-039 — Python Graft Host Binding](./ADR-039-python-graft-host-binding.md)
- [ADR-040 — Peer Authority Resolution (superseded by ADR-047)](./ADR-040-peer-authority-resolution.md)
- [ADR-041 — End-To-End Peer Write Outcome](./ADR-041-end-to-end-peer-write-outcome.md)
- [ADR-042 — SemVer Release And HTTP Compatibility](./ADR-042-semver-release-and-http-compatibility.md)
- [ADR-043 — Hermes Graft Wake-up Ownership and Recovery](./ADR-043-hermes-graft-wake-up-ownership.md)
- [ADR-044 — Public Verification Report Classification](./ADR-044-public-verification-report-classification.md)
- [ADR-045 — Runtime Observation Attribution](./ADR-045-runtime-observation-attribution.md)
- [ADR-046 — Template-Declared Workflow Metadata And Admission Snapshots](./ADR-046-template-declared-workflow-metadata.md)
- [ADR-047 — Durable Idle Search Projection](./ADR-047-durable-idle-search-projection.md)
- [ADR-049 — hermes-atm/atm-graft First Public PyPI Release Versioning](./ADR-049-hermes-atm-first-public-pypi-release-versioning.md)

## Extracted Crate-Local ADRs

- [ADR-ATM-RUNTIME-001 — `atm-runtime` As Concrete Composition Root](./ADR-ATM-RUNTIME-001.md)
- [ADR-ATM-RUSQLITE-002 — Single In-Process SQLite Write Worker](./ADR-ATM-RUSQLITE-002.md)

## Embedded Crate-Local ADR Records

These accepted ADR records are intentionally embedded in the crate architecture
documents for now and must be reviewed as part of those crate docs:

- `ADR-ATM-DAEMON-001` — `docs/atm-daemon/architecture.md`
  - daemon is the current runtime composition root
- `ADR-ATM-CORE-001` — `docs/atm-core/architecture.md`
  - shared ATM protocol lives in `atm-core`
- `ADR-ATM-001` — `docs/atm/architecture.md`
  - CLI uses shared protocol and client transport only
- `ADR-ATM-RUSQLITE-001` — `docs/atm-rusqlite/architecture.md`
  - concrete SQLite adapters remain private

Until they are extracted into standalone files, this index is the canonical
place that names and locates the crate-local ADR records.
