# ATM-Core Crate Requirements

## 1. Purpose

This document defines the `atm-core` crate requirements.

The `atm-core` crate owns the reusable daemon-free ATM business logic. Product
behavior remains defined in [`../requirements.md`](../requirements.md).

## 2. Ownership

`atm-core` owns:

- path and config resolution policy
- address parsing and validation
- mailbox I/O
- workflow and typestate rules
- send/read/ack/clear service behavior
- log query/follow service behavior over the observability boundary
- doctor service behavior
- structured core errors

`atm-core` does not own:

- clap parsing
- terminal formatting
- process exit policy
- direct dependency on concrete observability crates

## 3. Requirement Namespace

The `atm-core` crate uses the `REQ-CORE-*` namespace.

Initial allocation:

- `REQ-CORE-CONFIG-*`
- `REQ-CORE-MAILBOX-*`
- `REQ-CORE-WORKFLOW-*`
- `REQ-CORE-SEND-*`
- `REQ-CORE-READ-*`
- `REQ-CORE-ACK-*`
- `REQ-CORE-CLEAR-*`
- `REQ-CORE-LOG-*`
- `REQ-CORE-DOCTOR-*`
- `REQ-CORE-OBS-*`

Initial crate requirement IDs:

- `REQ-CORE-CONFIG-001` `atm-core` owns daemon-free home/path/config/identity
  resolution policy. Satisfies the path/config/identity aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-IDENTITY-001`, `REQ-P-DOCTOR-001`.
- `REQ-CORE-CONFIG-002` `atm-core` owns daemon-free address parsing,
  alias/role rewrite, and team/member validation policy. Satisfies the address
  resolution and target-validation aspects of:
  `REQ-P-ADDRESS-001`, `REQ-P-SEND-001`, `REQ-P-READ-001`,
  `REQ-P-CLEAR-001`.
- `REQ-CORE-CONFIG-003` `atm-core` owns persisted config/team schema recovery
  and diagnostic policy. Satisfies the compatibility-recovery and
  persisted-data error aspects of:
  `REQ-P-CONFIG-HEALTH-001`, `REQ-P-ERROR-001`,
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-SEND-001` `atm-core` owns send-time missing-config fallback,
  sender-warning, and repair-notification behavior above the shared config
  loader. Satisfies the missing-config send-path aspects of:
  `REQ-P-SEND-001`, `REQ-P-CONFIG-HEALTH-001`,
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-SEND-002` `atm-core` owns ATM-authored alert metadata placement,
  compatibility reads, and degradation rules across write/read paths. Satisfies
  the alert-metadata schema and sender-side dedup aspects of:
  `REQ-P-SCHEMA-001`, `REQ-P-CONFIG-HEALTH-001`,
  `REQ-P-RELIABILITY-001`.
- `REQ-CORE-MAILBOX-001` `atm-core` owns daemon-free mailbox/store behavior.
  Satisfies the persisted mailbox I/O and mutation aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-SEND-001`, `REQ-P-READ-001`,
  `REQ-P-ACK-001`, `REQ-P-CLEAR-001`, `REQ-P-RELIABILITY-001`.
- `REQ-CORE-WORKFLOW-001` `atm-core` owns the two-axis workflow model and legal
  transitions. Satisfies the state-classification and legal-transition aspects
  of:
  `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-WORKFLOW-001`.
- `REQ-CORE-LOG-001` `atm-core` owns ATM log query/follow service behavior over
  the injected observability boundary. Satisfies the core
  query/follow/filtering aspects of:
  `REQ-P-LOG-001`, `REQ-P-OBS-001`.
- `REQ-CORE-DOCTOR-001` `atm-core` owns local doctor diagnostics and readiness
  evaluation. Satisfies the diagnostic evaluation aspects of:
  `REQ-P-DOCTOR-001`, `REQ-P-OBS-001`.
- `REQ-CORE-OBS-001` `atm-core` owns the abstract observability boundary and
  ATM-owned event/query models above shared crates. Satisfies the ATM event,
  query-model, and health-contract aspects of:
  `REQ-P-OBS-001`.

## 4. Module Ownership

Per-module documentation lives under:

- [`modules/send.md`](./modules/send.md)
- [`modules/read.md`](./modules/read.md)
- [`modules/ack.md`](./modules/ack.md)
- [`modules/clear.md`](./modules/clear.md)
- [`modules/log.md`](./modules/log.md)
- [`modules/doctor.md`](./modules/doctor.md)
- [`modules/mailbox.md`](./modules/mailbox.md)
- [`modules/config.md`](./modules/config.md)
- [`modules/observability.md`](./modules/observability.md)

Each module document defines:

- service responsibility
- invariants
- inputs and outputs
- references to the product requirements it implements

## 5. Required References

The `atm-core` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../atm-message-schema.md`](../atm-message-schema.md)
- [`../legacy-atm-message-schema.md`](../legacy-atm-message-schema.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`./design/dedup-metadata-schema.md`](./design/dedup-metadata-schema.md)
- [`./design/sc-observability-integration.md`](./design/sc-observability-integration.md)

## 6. Send Alert Metadata

Requirement ID:
- `REQ-CORE-SEND-002`

Required write-path rules:
- ATM-authored alert field writes must use ATM-owned `metadata.atm` fields
- forward alert writes must target `metadata.atm.alertKind` and
  `metadata.atm.missingConfigPath` or a later explicitly documented
  `metadata.atm` field
- new ATM-only alert top-level fields must be rejected with a descriptive
  validation error on the write path
- exception: until the alert metadata migration sprint lands, the current
  runtime send path may continue writing legacy top-level `atmAlertKind` and
  `missingConfigPath` fields; this carve-out is bounded by
  [`architecture.md` §3.1](./architecture.md)
- the write-path rejection requirement applies to new ATM-only alert fields
  introduced after Phase J

Required read-path rules:
- ATM read must accept legacy top-level alert fields such as `atmAlertKind` and
  `missingConfigPath`
- ATM read must also accept forward `metadata.atm` alert fields
- malformed ATM-owned alert metadata must degrade gracefully, emit warning
  diagnostics, and never cause the message to be dropped when the
  Claude-native envelope remains usable

Forward migration rule:
- legacy top-level `atmAlertKind` migrates to `metadata.atm.alertKind`
- legacy top-level `missingConfigPath` migrates to
  `metadata.atm.missingConfigPath`
- the forward architectural target and compatibility-period carve-out are
  documented in [`architecture.md` §3.1](./architecture.md)

## 7. Observability Integration Boundary

Requirement ID:
- `REQ-CORE-OBS-001`

Required boundary rules:
- `atm-core` owns the injected observability boundary used by retained command
  services
- `atm-core` must not depend on concrete `sc-observability` crate types
- the boundary must cover emit, query, follow, and health rather than
  remaining emit-only
- ATM-owned projected request/result types must be defined in `atm-core` for:
  - log query
  - log record projection
  - tail-session projection
  - doctor health projection
- the boundary must remain synchronous and object-safe for service injection
- shared query/follow and health failures must map to stable `AtmErrorKind`
  variants without leaking shared error enums into `atm-core`
- `atm-core` command-service failures and degraded recovery warnings must expose
  stable ATM-owned error codes for the CLI observability adapter to log
- the corresponding source-of-truth code registry must live in one source file
  and match [`../atm-error-codes.md`](../atm-error-codes.md)

Detailed design and implementation shape is owned by:
- [`design/sc-observability-integration.md`](./design/sc-observability-integration.md)
