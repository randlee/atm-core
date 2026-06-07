# Phase AA Daemon State-Machine Inventory

## Purpose

Freeze the small auditable daemon-machine target before code deletion begins.

Phase AA target:
- no more than `5` top-level daemon state machines
- no backend-specific SQLite control flow in the daemon

## Target Machine Set

### 1. Bootstrap / Singleton Ownership

Owned concerns:
- launch gate
- host ownership acquisition
- transition into serving or clean failure

Primary files:
- `crates/atm-daemon/src/host_ownership.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`
- bootstrap portions of `crates/atm-daemon/src/composition.rs`

### 2. Request Receipt / Validation / Dispatch / Reply

Owned concerns:
- local IPC request acceptance
- request decoding
- dispatch to injected runtime/service ports
- bounded reply / error return

Primary files:
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/local_ipc_transport/request_worker.rs`
- temporary pre-`AA.3` dispatch portions of
  `crates/atm-daemon/src/runtime_health.rs`
  - this file is still listed below as a target violation because its current
    SQLite-aware health logic exceeds the desired machine boundary and must be
    deleted or split during the Phase AA line

### 3. Session / Connection Lifecycle

Owned concerns:
- active connection registration
- session open / active / draining / close
- advisory session registration lifecycle

Primary files:
- `crates/atm-daemon/src/active_connection_registry.rs`
- `crates/atm-daemon/src/local_ipc_connection.rs`
- `crates/atm-daemon/src/advisory_runtime.rs`

### 4. Graceful Shutdown / Drain

Owned concerns:
- runtime stop transition
- bounded drain
- shutdown beacon / finalizer ownership

Primary files:
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/shutdown_beacon.rs`

### 5. Advisory Stream Lifecycle

Owned concerns:
- one live stream per active session if that surface survives
- register / drain / close

Primary files:
- `crates/atm-daemon/src/advisory_runtime.rs`

## Current Modules That Violate The Target

These modules or function families are not part of the desired thin-router
machine set and must be removed, moved outward, or reduced to injected trait
calls:

- `crates/atm-daemon/src/sqlite_observability.rs`
  - backend-specific observability logic
- `crates/atm-daemon/src/runtime_health.rs`
  - direct SQLite health / roster access
- `crates/atm-daemon/src/runtime_status_cache.rs`
  - SQLite-named readiness fields and helpers
- `crates/atm-daemon/src/lib.rs`
  - daemon-owned SQLite replay wrapper
- `crates/atm-daemon/src/composition.rs`
  - concrete SQLite construction
- `crates/atm-daemon/src/runtime_health_test_support.rs`
  - daemon-local SQLite test assembly

## Review Rule

Any daemon code path that introduces:
- `SqliteBoundaryAssembly`
- `SqliteObservability`
- direct `atm_rusqlite::*` references
- backend-specific health or replay semantics

must be treated as outside the target machine inventory unless explicitly moved
behind an injected storage-neutral trait.
