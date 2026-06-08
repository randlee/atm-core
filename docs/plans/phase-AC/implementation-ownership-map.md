# Phase AC Implementation Ownership Map

## Goal

Map the current storage-facing trait implementations and major module owners to
the sprint that should absorb, replace, or delete them.

This is an `AC.0` planning collateral artifact used to route real execution
work without rediscovering where the current logic lives.

## Current Implementers

Current concrete trait implementers found during `AC.0`:

| Trait Surface | Current Implementer | Current Location | Planned Owner |
| --- | --- | --- | --- |
| `MailStore` | `SqliteMailStore` | `crates/atm-rusqlite/src/lib.rs` | `AC.3` |
| `MailStoreDoctor` | `SqliteMailStore` | `crates/atm-rusqlite/src/lib.rs` | `AC.3` |
| `TaskStore` | deleted speculative surface | former `crates/atm-core/src/boundary/store.rs` and runtime compile bridge | `AC.6` closed by deletion |
| `TaskStoreDoctor` | deleted speculative surface | former `crates/atm-core/src/boundary/store.rs` and doctor bridge | `AC.6` closed by deletion |
| `RosterStore` | `SqliteRosterStore` | `crates/atm-rusqlite/src/roster_store.rs` | `AC.3` |
| `RosterStoreDoctor` | `SqliteRosterStore` | `crates/atm-rusqlite/src/boundary_assembly.rs` | `AC.3` |
| `RemoteReplayStore` capability/internalization | `SqliteRemoteReplayStore` | `crates/atm-runtime/src/replay_store.rs` | `AC.3` |
| `RemoteReplayStore` consumer seam deletion | `SqliteRemoteReplayStore` | `crates/atm-runtime/src/replay_store.rs` | `AC.4` |
| `RuntimeStorageFinalizer` capability/internalization | `SqliteRuntimeStorageFinalizer` | `crates/atm-runtime/src/replay_store.rs` | `AC.3` |
| `RuntimeStorageFinalizer` consumer seam deletion | `SqliteRuntimeStorageFinalizer` | `crates/atm-runtime/src/replay_store.rs` | `AC.4` |
| Claude outbound write seam | `ProjectionMailboxWriter` trait plus runtime adapters | `crates/atm-core/src/delivery_execution.rs` | `AC.2` and `AC.4` |
| Non-Claude outbound | `DaemonNonClaudeOutbound` | `crates/atm-daemon/src/non_claude_outbound_runtime.rs` | outside core `AC` storage reset scope unless trait convergence requires touch-up |

## Claude Storage Module Ownership

Current Claude mailbox implementation modules:

- `crates/atm-core/src/mailbox/mod.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/src/mailbox/source.rs`
- `crates/atm-core/src/mailbox/atomic.rs`
- `crates/atm-core/src/mailbox/lock.rs`
- `crates/atm-core/src/mailbox/hash.rs`
- `crates/atm-core/src/mailbox/surface.rs`

Planned handling:

- `AC.2` evaluates these as the candidate extraction set for
  `atm-storage-claude`
- `surface.rs` may remain shared logic only if it is truly backend-agnostic
- lock / atomic rewrite / salvage mechanics belong below the Claude backend
  trait line

## Duplicate-Type Pressure Points

Current high-pressure record families that later sprints must collapse:

- `MailStoreMessageRecord` <-> `MessageEnvelope` / logical delivery records
- `RosterMemberRecord` <-> `ProjectionRosterMember` / `ProjectionRoster`
- speculative task-store wrappers and records that should not be preserved as
  approved `AC` contract surface

Representative consumers are spread broadly through:

- `ack/`
- `read/`
- `clear/`
- `team_admin/`
- `delivery_execution.rs`
- `service_runtime.rs`
- `send/`
- `atm-rusqlite`

Planning consequence:

- `AC.1` must define the canonical message/roster shared types before `AC.4`
  and `AC.5` attempt consumer migration
- speculative task-store code is not an input to the shared contract and is
  instead routed to deletion/quarantine closeout

## Sprint Routing

### AC.1

Owns:

- choosing the canonical shared type set
- deciding which current boundary types survive only as semantic query / key
  helpers

### AC.2

Owns:

- deciding which mailbox modules move fully into `atm-storage-claude`
- deciding which helper logic, if any, is truly backend-agnostic

### AC.3

Owns:

- moving the SQLite backend behind `atm-storage`
- reducing `atm-rusqlite` dependence on `atm-core`
- making replay/finalizer seams compatible with the new storage model
- deciding whether replay/finalizer seams survive as backend-internal details
  or named optional capabilities
- explicitly not treating SQLite task persistence as approved shared-contract
  scope

### AC.4

Owns:

- consumer migration in `atm-core`
- deleting consumer-side replay/finalizer and other backend-shaped seams after
  shared contract adoption

### AC.5

Owns:

- collapsing RPC body duplication onto the canonical shared types

### AC.6

Owns:

- residual deletion closeout for old wrappers and backend leakage
- speculative task-store deletion by default so later phases do not inherit
  accidental legacy assumptions; quarantine is only a fallback if removal is
  concretely blocked

### AC.7

Owns:

- SQL Server readiness proof against the final post-cleanup contract
