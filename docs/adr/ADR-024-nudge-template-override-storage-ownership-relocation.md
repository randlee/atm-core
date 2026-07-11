# ADR-024 — NudgeTemplateOverrideStore Storage Ownership Relocation

| Field | Value |
|---|---|
| ID | ADR-024 |
| Status | **Accepted** |
| Date | 2026-07-11 |
| Deciders | Rand Lee |
| Relates to | ADR-018, ADR-021, ADR-022 |
| Supersedes | ADR-021 |

---

## Context

`AD.21` introduced the built-in nudge override contract under
`atm_core::boundary`, then widened its dependents under ADR-021. That shape
worked for compile-bridge consumers, but it left a real forbidden production
edge in place:

- `atm-storage-rusqlite -> atm-core`

The SQLite backend had to depend on `atm-core` to implement and return the
override-store contract, and mailbox metadata reconstruction also reached into
`atm-core` for the durable ack classifier. That contradicted ADR-018's storage
contract reset, which requires storage backends to depend only on shared
storage-neutral contracts rather than upward business facades.

## Decision

Move canonical ownership of the following storage-neutral contract family from
`atm-core` to `atm-storage`:

- `NudgeTemplateOverrideStore`
- `BuiltInNudgeTemplateKind`
- `TeamNudgeTemplateOverrideMode`
- `TeamNudgeTemplateOverrideRow`
- `AckRequirementState`
- `derive_ack_requirement(...)`

Keep two explicit compatibility rules:

1. `atm-core` may continue to re-export the moved trait and storage-neutral
   rows during the compile-bridge cutover.
2. `atm-core` retains the helper
   `built_in_nudge_template_kind_from_post_send_event(...)` because
   `PostSendHookEvent` remains `atm-core` owned and that helper cannot move
   without reintroducing an `atm-storage -> atm-core` edge or an orphan-rule
   conflict.

## Sealing

The moved override-store contract remains sealed. `atm-storage` now owns the
`sealed` module for `NudgeTemplateOverrideStore`, and concrete backends such as
`atm-storage-rusqlite` implement that `atm-storage` seal directly.

This intentionally creates a temporary asymmetry with older `atm-storage`
traits such as `MessageStore` and `RosterStore`, which remain unsealed. That
asymmetry is accepted here rather than silently weakening the moved contract.

## Consequences

### Positive

- `atm-storage-rusqlite` no longer needs a normal dependency on `atm-core`
- the durable ack classifier now sits beside the canonical message schema
- backend-neutral override-store types live in the shared storage contract
  crate where backends can implement them without boundary leakage

### Negative

- `atm-storage`'s package-level boundary record must allow the broader direct
  dependent set imposed by existing Cargo edges, even though only a subset of
  those crates consume this specific contract today
- `atm-core` carries a temporary compatibility re-export surface until all
  retained callers migrate

## Review Conditions

This ADR remains valid only while all of the following stay true:

- `atm-storage-rusqlite` has no production imports from `atm-core`
- `atm-core` retains only the compatibility re-export plus the
  `PostSendHookEvent`-specific helper for this contract family
- built-in template rendering and product wording remain outside `atm-storage`
- concrete backends keep implementing the `atm-storage` seal directly
