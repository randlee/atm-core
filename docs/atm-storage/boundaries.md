# ATM-Storage Boundary Inventory

This document records shared storage-neutral contracts owned by `atm-storage`.

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
