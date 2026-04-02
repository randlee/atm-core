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
- `REQ-POST-SEND-*`

Initial crate requirement IDs:

- `REQ-CORE-CONFIG-001` `atm-core` owns daemon-free home/path/config/identity
  resolution policy. Satisfies the path/config/identity aspects of:
  `REQ-P-CONTRACT-001`, `REQ-P-IDENTITY-001`, `REQ-P-DOCTOR-001`.
- `REQ-CORE-CONFIG-002` `atm-core` owns daemon-free address parsing,
  alias/role rewrite, and team/member validation policy. Satisfies the address
  resolution and target-validation aspects of:
  `REQ-P-ADDRESS-001`, `REQ-P-SEND-001`, `REQ-P-READ-001`,
  `REQ-P-CLEAR-001`.
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

## 4. Post-Send Hook Requirements

The post-send hook is an `atm-core` extension on top of the retained send path.
It remains daemon-free, optional, and per-recipient-agent.

- `REQ-POST-SEND-001` `atm-core` must support an optional per-agent
  `post_send` bash command in `.atm.toml` under `[agents.<name>]`. This
  extends the config ownership of `REQ-CORE-CONFIG-001` without changing the
  existing identity and default-team precedence rules. Satisfies the config
  extensibility aspects of: `REQ-P-POST-SEND-001`.
- `REQ-POST-SEND-002` `atm-core` must evaluate the `post_send` hook only after
  the send path has durably written the outbound message to the recipient inbox.
  The hook is a post-success side effect and must not run for dry-run sends or
  failed writes. In the planned send-path refactor, this boundary is the
  `WriteOutcome::Success` point. Satisfies the ordering aspects of:
  `REQ-P-POST-SEND-001`.
- `REQ-POST-SEND-003` `atm-core` must execute the configured `post_send` hook
  as a fire-and-forget child process. Hook spawn, launch, and process errors
  must not convert a successful send into a send failure and must not roll back
  the already-written mailbox message. Satisfies the error-boundary aspects of:
  `REQ-P-POST-SEND-001`.
- `REQ-POST-SEND-004` `atm-core` must execute the hook with `sh -c` and expose
  the send context through environment variables set on the spawned command:
  `ATM_SENDER`, `ATM_RECIPIENT`, `ATM_MESSAGE_BODY`, and `ATM_MESSAGE_ID`.
  Satisfies the hook-context aspects of: `REQ-P-POST-SEND-001`.
- `REQ-POST-SEND-005` `atm-core` must resolve `post_send` from the recipient's
  agent config entry. A missing `[agents.<name>]` table or missing `post_send`
  field means no hook is executed for that recipient. Satisfies the
  recipient-scoped behavior aspects of: `REQ-P-POST-SEND-001`.

## 5. Module Ownership

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

## 6. Required References

The `atm-core` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
