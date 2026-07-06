---
id: AD.13
title: ULID Message Identity Reset
status: planned
branch: feature/pAD-s13-ulid-message-identity-reset
worktree: ../atm-core-worktrees/feature/pAD-s13-ulid-message-identity-reset
target: integrate/phase-AD
---

# Sprint AD.13 — ULID Message Identity Reset

## Goal

- remove all retained UUID usage from the accepted ATM code path so message
  identity, schema/tooling, and supporting uniqueness helpers are ULID-only

## Hard Dependencies

- `AD.12` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`

## Exact Targets

- `Cargo.toml`
- `Cargo.lock`
- `crates/atm-core/Cargo.toml`
- `crates/atm-core/src/mailbox/mod.rs`
- `crates/atm-core/src/persistence.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/schema/inbox_message.rs`
- `crates/atm-core/tests/mailbox_locking.rs`
- `crates/atm-storage/Cargo.toml`
- `crates/atm-storage/src/schema/inbox_message.rs`
- `crates/atm-storage-rusqlite/src/writer/ops.rs`
- `tools/schema_models/atm_message_schema.py`
- `tools/schema_models/test_schema_models.py`
- `docs/adr/ADR-012-one-message-identity.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

## Interfaces To Add Or Modify

The accepted ATM message-id boundary after this sprint is:

```rust
pub struct AtmMessageId(Ulid);

impl FromStr for AtmMessageId {
    type Err = AtmError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Ulid::from_str(raw)
            .map(Self)
            .map_err(|_| AtmError::invalid_message_id("expected ULID"))
    }
}
```

Retained serialized message-id fields are ULID text only:

```rust
pub struct InboxMessage {
    pub message_id: AtmMessageId,
    pub parent_message_id: Option<AtmMessageId>,
}
```

## Paths To Delete

- the `uuid` workspace dependency and all crate-local `uuid` dependencies
- `AtmMessageId::from_uuid_wire(...)`
- `AtmMessageId::into_uuid_wire(...)`
- `impl From<Uuid> for AtmMessageId`
- `impl From<AtmMessageId> for Uuid`
- UUID parse fallback in retained `AtmMessageId::from_str` paths
- UUID-wire serde helpers for retained inbox message schemas
- UUID-typed message-id fields and tests in `tools/schema_models/*`
- retained CLI/runtime error text that advertises UUID-form ATM message ids as
  valid accepted input
- UUID-backed mailbox/test helper construction where ULID generation is
  sufficient
- UUID temp-file suffix generation in `crates/atm-core/src/persistence.rs`
- UUID-compatible service-addressing or boundary-encoding claims from
  `docs/requirements.md`, `docs/architecture.md`,
  `docs/atm-core/architecture.md`, and `ADR-012`

## Deliverables

- retained ATM message identity is ULID-only across code, tooling, and
  accepted docs
- the workspace no longer depends on `uuid`
- retained tests and fixtures generate ULIDs instead of UUIDs
- persistence helpers no longer use UUIDs for uniqueness

## This Sprint Does Not Close

- graft boundary reset
- daemon advisory runtime deletion
- `atm-graft` receiver reset
- final smoke/readiness closeout

## Acceptance Criteria

- no retained ATM code path parses, emits, serializes, stores, or documents
  UUID-form message ids as accepted input/output
- `Cargo.toml`, `Cargo.lock`, `crates/atm-core/Cargo.toml`, and
  `crates/atm-storage/Cargo.toml` contain no `uuid` dependency
- retained uniqueness helpers use ULID or another non-UUID mechanism, with no
  remaining `uuid::` imports in the workspace
- `ADR-012`, `docs/requirements.md`, `docs/architecture.md`, and
  `docs/atm-core/requirements.md`, and `docs/atm-core/architecture.md`
  describe ULID-only retained ATM message identity
- no accepted implementation or doc path tells developers to preserve
  UUID/ULID bridge code

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `rg -n "from_uuid_wire|into_uuid_wire|uuid::|\\bUuid\\b|\\buuid\\b|ULID or UUID|UUID wire|UUID-formatted --message-id|UUID-based suffixes" crates tools Cargo.toml Cargo.lock docs/requirements.md docs/architecture.md docs/atm-core/architecture.md docs/adr/ADR-012-one-message-identity.md`
- `git diff --check`
