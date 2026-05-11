# ATM-Graft Boundary Inventory

This document is the crate-local boundary inventory for `atm-graft`.

`atm-graft` consumes public protocol/session boundaries owned by `atm-core`
and must remain a thin embedded client crate rather than a second runtime or
business-logic layer.

## AtmGraftClient Consumer

Purpose:
- consume the public unary daemon-client boundary owned by `atm-core`
- provide concrete embedded same-host client behavior for `send`, `read`, and
  `ack`

Rules:
- `atm-graft` must not take a Rust dependency on `atm-daemon`
- `atm-graft` may depend on `atm-daemon-client` only for shared same-host
  daemon bootstrap helpers; transport and session contracts remain `atm-core`
  owned
- `atm-graft` must not re-mint a parallel public client trait duplicating
  `atm_core::AtmGraftClient`

## GraftSessionPort Consumer

Purpose:
- consume the public session boundary owned by `atm-core`
- provide the concrete `GraftSession` implementation used by a custom host CLI

Rules:
- `atm-graft` must not define a parallel public session trait that duplicates
  `atm_core::GraftSessionPort`
- `atm-graft` must not own daemon queue state, direct SQLite access, or direct
  inbox-JSONL access
- automatic between-tool-call nudge injection belongs to this consumer layer,
  but daemon-owned queue state remains outside it
