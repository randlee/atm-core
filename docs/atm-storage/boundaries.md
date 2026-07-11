# ATM-Storage Boundary Inventory

This document records shared storage-neutral contracts owned by `atm-storage`.

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
