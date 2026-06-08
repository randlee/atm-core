# Phase AC Storage Surface Inventory

## Goal

Record the concrete storage-facing surface that Phase `AC` must shrink and
restructure before implementation sprints begin.

This document is an `AC.0` planning collateral artifact. Later sprints use it
to decide what gets moved, merged, or deleted rather than re-inventing the
inventory ad hoc.

## Baseline Commands

Primary baseline commands used for this inventory:

```bash
rg -n "pub trait|pub struct|pub enum" \
  crates/atm-core/src/boundary \
  crates/atm-core/src/delivery_execution.rs -S

rg -n "Request|Response|ClaudeInboxWriter|InboxIngress|MailStore|TaskStore|RosterStore" \
  crates/atm-core/src/boundary \
  crates/atm-core/src/mailbox \
  crates/atm-core/src/delivery_execution.rs -S

rg -n "atm-rusqlite|rusqlite|sqlite" \
  crates/atm-core crates/atm-daemon crates/atm-runtime crates/atm-rusqlite -S
```

## Surface Summary

The accepted `AC.0` baseline is:

- roughly `13` storage-adjacent public traits
- roughly `95` storage-adjacent public structs
- roughly `3` public enums

This is too large for the intended shared `atm-storage` contract.

Breakdown of the main boundary files:

| File | Traits | Structs | Enums |
| --- | ---: | ---: | ---: |
| `crates/atm-core/src/boundary/mail.rs` | `2` | `35` | `0` |
| `crates/atm-core/src/boundary/store.rs` | `9` | `58` | `3` |
| `crates/atm-core/src/boundary/runtime.rs` | `2` | `2` | `0` |

Request / response wrapper counts in the two main storage boundary files:

- `boundary/mail.rs`: `26`
- `boundary/store.rs`: `48`

## Main Sources

Primary overgrown storage/RPC boundary sources:

- `crates/atm-core/src/boundary/mail.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary/runtime.rs`
- `crates/atm-core/src/delivery_execution.rs`

Secondary concrete Claude storage / mailbox sources:

- `crates/atm-core/src/mailbox/mod.rs`
- `crates/atm-core/src/mailbox/store.rs`
- `crates/atm-core/src/mailbox/source.rs`
- `crates/atm-core/src/mailbox/lock.rs`

Concrete SQLite backend sources:

- `crates/atm-rusqlite/src/lib.rs`
- `crates/atm-rusqlite/src/shared_db.rs`
- `crates/atm-rusqlite/src/roster_store.rs`
- `crates/atm-rusqlite/src/mailbox_metadata.rs`
- `crates/atm-rusqlite/src/writer/ops.rs`

Runtime / daemon coupling sources:

- `crates/atm-runtime/src/composition.rs`
- `crates/atm-runtime/src/replay_store.rs`
- `crates/atm-runtime/src/sqlite_observability.rs`
- `crates/atm-daemon/src/runtime_sqlite_observer.rs`

## Overgrowth Categories

### 1. Request / Response DTO Proliferation

Representative families:

- `MailStore*Request` / `MailStore*Response`
- `TaskStore*Request` / `TaskStore*Response`
- `RosterStore*Request` / `RosterStore*Response`
- `InboxIngress*Request` / `InboxIngress*Response`
- `InboxExport*Request` / `InboxExport*Response`

Measured baseline:

- at least `74` explicit `*Request` / `*Response` wrapper structs across
  `boundary/mail.rs` and `boundary/store.rs` alone

Planning consequence:

- `AC.1` must not lift these families into `atm-storage`
- they are raw deletion / collapse candidates unless a true semantic query or
  mutation type survives the redesign
- the `TaskStore*Request` / `TaskStore*Response` families are inventoried here
  because they contribute to the current oversized surface, but Phase `AC`
  treats them as speculative task-storage scaffolding routed to deletion or
  quarantine in `AC.6`, not as approved shared-contract inputs
- `docs/plans/phase-AC/type-ledger.md` is the exhaustive manifest that names each
  current type and its planned disposition

### 2. Claude Storage Seams Outside A Shared Storage Contract

Representative seams:

- `ClaudeInboxWriter` in `delivery_execution.rs`
- mailbox file append / rewrite / salvage helpers
- inbox ingress / export split around Claude-specific files

Planning consequence:

- `AC.2` must extract Claude storage as a real backend implementation rather
  than preserving these as ad hoc compatibility seams in `atm-core`

### 3. SQLite Leakage Above The Trait Line

Representative evidence:

- direct `sqlite` / `rusqlite` wording and state-machine names in `atm-core`
- runtime assembly built around concrete SQLite boundary types
- daemon SQLite observer wiring

Planning consequence:

- `AC.3` and `AC.4` must pull concrete SQLite behavior below the shared
  contract and off the higher-level semantic surfaces

## Required Use In Later Sprints

- `AC.1` uses this inventory to justify the reduced trait and type surface
- `AC.2` uses this inventory to identify Claude-owned modules
- `AC.3` uses this inventory to identify SQLite-owned modules and coupling
- `AC.6` uses this inventory as a delete/keep checklist
