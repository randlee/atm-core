# ATM-Storage Boundary Inventory

This document records shared storage-neutral contracts owned by `atm-storage`.

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
- own backend-neutral durable records for enabled peer HTTP interfaces, exact
  trusted peer hostnames/ports, and explicit canonical aliases

Rules:
- this contract must not perform socket I/O, TLS, DNS, delivery attempts,
  retries, peer scans, timers, or outbound worker coordination
- `atm-runtime`, `atm-daemon-bootstrap`, and the CLI consume the contract;
  concrete backends implement it without leaking backend details upstream

## MessageStore peer confirmation

`MessageStore::confirm_peer_delivery` is the sole storage mutation following a
matching direct peer HTTP response. It removes the matching `peerOutbound`
marker and leaves the immutable message, ACK/read state, and mailbox history
unchanged. A failed direct attempt retains that marker; storage creates no
outbox, receipt table, or delivery-state machine.

`OutboundMessageQuery` may select the immutable retained records for the
ADR-046 in-memory resend aggregate. `pending_peer_hosts` is one deterministic,
read-only distinct-host bootstrap query; `page_for_peer` is cursor-only,
oldest-first and never applies an age policy. Neither contract schedules,
connects, resolves DNS, mutates delivery state, or owns a timer.

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
