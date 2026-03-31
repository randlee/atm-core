# ATM Crate Requirements

## 1. Purpose

This document defines the `atm` crate requirements.

The `atm` crate owns the CLI layer only. Product behavior remains defined in
[`../requirements.md`](../requirements.md). `atm` must satisfy those product
requirements without re-owning `atm-core` business logic.

## 2. Ownership

`atm` owns:

- clap command parsing
- command dispatch
- user-facing output rendering
- process exit behavior
- one-time observability bootstrap
- concrete implementation of the `atm-core` observability boundary

`atm` does not own:

- mailbox mutation logic
- state-machine logic
- config resolution policy
- log query business logic
- doctor business logic

## 3. Requirement Namespace

The `atm` crate uses the `REQ-ATM-*` namespace.

Initial allocation:

- `REQ-ATM-CMD-*` for command-entry requirements
- `REQ-ATM-OUT-*` for output/rendering requirements
- `REQ-ATM-OBS-*` for observability-bootstrap requirements

Initial crate requirement IDs:

- `REQ-ATM-CMD-001` `atm` owns clap parsing and command dispatch for the
  retained command surface. Satisfies:
  `REQ-P-SEND-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`.
- `REQ-ATM-OUT-001` `atm` owns human-readable and JSON rendering for retained
  commands. Satisfies:
  `REQ-P-SEND-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`.
- `REQ-ATM-OBS-001` `atm` owns concrete observability bootstrap and injection
  into `atm-core`. Satisfies:
  `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`, `REQ-P-OBS-001`.

## 4. Command Ownership

Per-command documentation lives under:

- [`commands/send.md`](./commands/send.md)
- [`commands/read.md`](./commands/read.md)
- [`commands/ack.md`](./commands/ack.md)
- [`commands/clear.md`](./commands/clear.md)
- [`commands/log.md`](./commands/log.md)
- [`commands/doctor.md`](./commands/doctor.md)

Each command document defines:

- CLI-owned flags and parsing rules
- CLI-to-core mapping
- output rendering behavior
- references to the relevant product and `atm-core` requirements

## 5. Required References

The `atm` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
