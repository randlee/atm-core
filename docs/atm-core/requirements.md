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
