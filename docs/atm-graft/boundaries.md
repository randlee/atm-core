# ATM-Graft Boundary Inventory

This document is the crate-local boundary inventory for `atm-graft`.

`atm-graft` consumes shared protocol and transport boundaries owned by
`atm-core` and must remain a thin embedded client crate rather than a second
runtime or business-logic layer.

Canonical machine-readable boundary source:
- [../../boundaries/atm-graft/shared-client-consumer.toml](../../boundaries/atm-graft/shared-client-consumer.toml)
- [../../boundaries/atm-graft/message-received-hook.toml](../../boundaries/atm-graft/message-received-hook.toml)

## Shared Client Transport Consumer

Purpose:
- consume the shared thin-client daemon request boundary owned by `atm-core`
- consume the shared same-host bootstrap seam owned by `atm-daemon-client`
- provide concrete embedded same-host client behavior for `send`, `read`, and
  `ack`

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
- own any receiver-private activation, wakeup, and temporary buffering needed
  to hand post-send events to the host
- drive host wake/event callback on arrival

Rules:
- `atm-graft` must not own direct SQLite access or direct inbox-JSONL access
- automatic between-tool-call nudge injection belongs to this consumer layer
- reconnect and shutdown behavior are owned here rather than in daemon-private
  runtime code
- one active receiver must own each canonical `(graft root, team, agent)`
  endpoint record. Ownership acquisition is explicit and a second live owner
  fails without replacing the published endpoint; stale owner recovery is
  process-death-safe.
- receiver-local state may be a bounded transient nudge handoff only. It must
  not persist mail, retain acknowledgement state, or implement reconciliation.
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
- a Hermes adapter must use the host's non-interrupting steer insertion path,
  not normal user-message ingress; the latter may interrupt a running agent
- external terminal automation is not an accepted production delivery path
- a language binding may translate the existing `HostNudgeInjector` callback
  into its host language, but may not add another receiver, transport, retry,
  or routing path

## Message Received Hook

Purpose:
- receive a receiver-private, capability-authenticated loopback nudge and hand
  it to the embedding host

Rules:
- it is not a second daemon request path; graft `send`, `read`, and `ack` use
  the shared daemon HTTP client
- `interprocess::local_socket` and Windows named-pipe references are forbidden
  inside `atm-graft`; a direct Cargo edge to `interprocess` is forbidden as
  well. Unix UDS support is permitted only transitively through the approved
  `atm-daemon-client` facade.
- it must not dispatch daemon requests or access SQLite/storage directly
- endpoint records carry the receiver generation needed to make close
  compare-and-remove safe; they remain one receiver endpoint per current
  `(root, team, agent)`, not a multi-chat session registry
