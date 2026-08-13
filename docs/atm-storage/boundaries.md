# ATM-Storage Boundary Inventory

This document records shared storage-neutral contracts owned by `atm-storage`.

## TemplateCatalogStore

Canonical machine-readable boundary source:
- [../../boundaries/atm-storage/template-catalog-store.toml](../../boundaries/atm-storage/template-catalog-store.toml)

Purpose:
- sealed, backend-neutral immutable template catalog and one atomic decomposed
  message admission transition

Contract:
- request DTOs are `TemplateRegistration`, `TemplateListFilter`, and
  `DecomposedMessageAdmission`; AN.9/AN.10 add validated leaf
  `WorkflowSnapshot` and `MessageTagProvenance` to that admission payload
- response DTOs are `StoredTemplate`, `TemplateSummary`, and
  `DecomposedMessageAdmissionOutcome`
- `atm-core` resolves/validates workflow variables before this boundary;
  implementations persist only the validated leaf DTOs atomically with the
  decomposed message
- the boundary must not accept renderer handles, SQL, FTS syntax, concrete
  SQLite values, or an unsealed extension point; ADR-036 permits no additional
  storage capability trait for this work

Status:
- active; AN.9/AN.10 contract extension governed by ADR-046

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
