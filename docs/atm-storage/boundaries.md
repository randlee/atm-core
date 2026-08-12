# ATM-Storage Boundary Inventory

This document records shared storage-neutral contracts owned by `atm-storage`.

## AsyncMessageStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-storage/async-message-store.toml](../../boundaries/atm-storage/async-message-store.toml)

`AsyncMessageStore` is the Tokio daemon's narrow durable-admission contract.
It awaits bounded submission and a durable result without exposing SQLite,
transactions, writer threads, or queue implementation. The concrete SQLite
adapter owns one synchronous transaction thread; `atm-http-runtime` must use
this contract for writes and must not introduce `spawn_blocking` for admission.
`MessageStore` remains the temporary synchronous compatibility surface for
non-Tokio callers until the migration is performance-proven.

## MessageSearchStore and AsyncMessageSearchStore

Canonical machine-readable boundary sources:
- [../../boundaries/atm-storage/message-search-store.toml](../../boundaries/atm-storage/message-search-store.toml)
- [../../boundaries/atm-storage/async-message-search-store.toml](../../boundaries/atm-storage/async-message-search-store.toml)

`MessageSearchStore` owns the sealed, backend-neutral query, filter,
aggregate, page, and durable-result DTOs. `AsyncMessageSearchStore` is its
Tokio-safe companion, with the same semantics and a bounded deadline. Neither
trait exposes SQL, FTS syntax, renderer handles, or HTTP DTOs. The concrete
SQLite adapter owns FTS5/JSON1 compilation and a bounded reader lane; HTTP
consumers only await this port.

## AnalystQueryStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-storage/analyst-query-store.toml](../../boundaries/atm-storage/analyst-query-store.toml)

`AnalystQueryStore` is a separate, local-only read interface for the
`atm-query-python` Maturin binding. It is not a widening of the daemon's
`MessageSearchStore`: the binding is the only non-storage consumer, the
concrete SQLite adapter owns connection authorization and query budgets, and
the contract exposes neither SQLite handles nor any write or network operation.

## TlsHelpers

Canonical machine-readable boundary source:
- [../../boundaries/atm-storage/tls.toml](../../boundaries/atm-storage/tls.toml)

`atm_storage::tls` owns the canonical certificate parsing, fingerprint
normalization, rustls provider selection, trusted-peer pinning, and TLS 1.2/1.3
signature-verification helpers used by the live daemon adapter and the inactive
interop fixture. This is protocol verification and certificate admission, not
just value validation: the storage crate owns no socket I/O, listener, sender,
route, retry, or daemon lifecycle. The inactive
`atm-peer-tls-interop` crate consumes these values for its bounded curl mTLS
proof; its dependency is explicitly allowed by the helper boundary and it has
no production delivery capability.

## PeerConfigStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-storage/peer-config-store.toml](../../boundaries/atm-storage/peer-config-store.toml)

Purpose:
- own backend-neutral durable records for enabled HTTPS interfaces, the local
  certificate reference, and exact trusted peers

Rules:
- this contract must not perform socket I/O, TLS, delivery, retry, or message
  state management
- `atm-runtime`, `atm-daemon-bootstrap`, and the CLI consume the contract;
  concrete backends implement it without leaking backend details upstream

## NudgeTemplateOverrideStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-storage/nudge-template-override-store.toml](../../boundaries/atm-storage/nudge-template-override-store.toml)

Purpose:
- own the storage-neutral lookup contract for team-scoped built-in nudge
  template override rows
- own the durable ack-requirement classifier shared by storage backends and
  retained runtime projections

Rules:
- `atm-storage` owns `NudgeTemplateOverrideStore`,
  `BuiltInNudgeTemplateKind`, `TeamNudgeTemplateOverrideMode`,
  `TeamNudgeTemplateOverrideRow`, `AckRequirementState`, and
  `derive_ack_requirement`
- `atm-core` may re-export those moved contracts and rows as a temporary
  compile bridge, but it no longer owns the canonical contract
- `atm-core` retains only
  `built_in_nudge_template_kind_from_post_send_event(...)` because
  `PostSendHookEvent` remains core-owned
- `atm-storage` must not grow direct SQLite access, daemon business logic, or
  template rendering
- concrete backend implementations such as `atm-storage-rusqlite` must
  implement the `atm-storage` sealed trait directly and must not depend on
  `atm-core` to satisfy this contract
