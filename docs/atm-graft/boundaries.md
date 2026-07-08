# ATM-Graft Boundary Inventory

This document is the crate-local boundary inventory for `atm-graft`.

`atm-graft` consumes shared protocol and transport boundaries owned by
`atm-core` and must remain a thin embedded client crate rather than a second
runtime or business-logic layer.

Canonical machine-readable boundary source:
- [../../boundaries/atm-graft/shared-client-consumer.toml](../../boundaries/atm-graft/shared-client-consumer.toml)

## Shared Client Transport Consumer

Purpose:
- consume the shared thin-client daemon request boundary owned by `atm-core`
- consume the shared same-host bootstrap seam owned by `atm-daemon-client`
- provide concrete embedded same-host client behavior for `send`, `read`,
  `ack`, and `list`

Rules:
- `atm-graft` must not take a Rust dependency on `atm-daemon`
- `atm-graft` must not take a Rust dependency on `atm-daemon-bootstrap`
- `atm-graft` must use the shared `atm-core` request/response DTO family rather
  than inventing a graft-private daemon API
- `atm-graft` must not add a graft-specific public trait family if the shared
  `ClientTransport` boundary is sufficient

## Session Runtime Consumer

Purpose:
- own the concrete `GraftSession` lifecycle used by an embedded host CLI
- keep one receiver-local poll thread active while the session is enabled
- translate durable unread messages into host-consumable `PostSendHookEvent`
  values

Rules:
- `atm-graft` must not own daemon queue state, direct SQLite access, or direct
  inbox-JSONL access
- receiver-local state is limited to lifecycle state plus the transient
  delivered-id set needed to avoid reinjecting the same unread message
- automatic between-tool-call nudge injection belongs to this consumer layer
- session runtime code must use the shared unary ATM protocol only
- no daemon-owned advisory registration, fetch/drain, or stream/session
  protocol may cross this boundary

## Host Injection Consumer

Purpose:
- bridge received nudges into the embedding host at the next safe
  between-tool-call insertion point

Rules:
- the host executable owns the final insertion point
- `atm-graft` must drive that path automatically once nudges arrive
- the injected payload type is `atm_core::boundary::PostSendHookEvent`
- external terminal automation is not an accepted production delivery path
