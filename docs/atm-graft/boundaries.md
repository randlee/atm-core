# ATM-Graft Boundary Inventory

This document is the crate-local boundary inventory for `atm-graft`.

`atm-graft` consumes the shared HTTP application contract and must remain a
thin embedded client crate rather than a second runtime or business-logic layer.

Canonical machine-readable boundary source:
- [../../boundaries/atm-graft/shared-client-consumer.toml](../../boundaries/atm-graft/shared-client-consumer.toml)

## Shared Client Transport Consumer

Purpose:
- consume the shared thin-client daemon request boundary owned by `atm-core`
- consume the shared same-host bootstrap seam owned by `atm-daemon-client`
- provide concrete embedded same-host client behavior for `send`, `read`, and
  `ack`

Rules:
- `atm-graft` must not take a Rust dependency on `atm-daemon`
- `atm-graft` must not take a Rust dependency on `atm-daemon-bootstrap`
- `atm-graft` must use the shared route-specific request/response DTOs rather
  than inventing a graft-private daemon API
- `atm-graft` must not add a graft-specific public trait family when the shared
  HTTP client contract is sufficient

## Session Runtime Consumer

Purpose:
- own the concrete `GraftSession` lifecycle used by an embedded host CLI
- own any receiver-private activation, wakeup, and temporary buffering needed
  to hand post-send events to the host
- drive host wake/event callback on arrival

Rules:
- `atm-graft` must not own direct SQLite access or direct inbox-JSONL access
- automatic between-tool-call nudge injection belongs to this consumer layer
- reconnect and shutdown behavior are owned here rather than in daemon-private
  runtime code
- receiver-private task/thread/callback choices stay inside this consumer layer
- `atm-graft` must not require shared daemon session registration, daemon-owned
  per-session queues, or a dedicated shared advisory-stream packet family

## Host Injection Consumer

Purpose:
- bridge received nudges into the embedding host at the next safe
  between-tool-call insertion point

Rules:
- the host executable owns the final insertion point
- `atm-graft` must drive that path automatically once nudges arrive
- external terminal automation is not an accepted production delivery path
