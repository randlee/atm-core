# ATM Daemon Observability Boundary

## Purpose

This document defines the final Phase V.1 daemon observability boundary.

It is the design contract that `V.2` and `V.3` implement:
- `V.2` moves event ownership into daemon subsystems
- `V.3` deletes the old central mapping path

Carry-forward obligations:
- `ARCH-PU-002` and `RULE-002` remain open obligations, not closed historical
  notes
- `V.2` is the CI-enforcement sprint for this boundary
- `V.3` is the deletion and closure sprint for the old central mapper path

## Bottom-Of-Stack Rule

Observability is bottom-of-stack.

Hard boundary:
- the shared daemon observability layer imports no daemon subsystem types
- daemon subsystems depend on the injected observability trait
- observability emits already-shaped daemon event payloads; it does not
  reconstruct subsystem meaning after the fact

Forbidden:
- daemon-wide semantic reconstruction of subsystem events
- subsystem-specific event enums or structs imported into the shared
  observability adapter
- daemon-global logger state that caches `team`, `message_id`, or `task_id`
  across unrelated events

## Injected Trait Shape

The active replacement daemon injects the existing object-safe, sealed core
port directly:

```rust
Arc<dyn ObservabilityPort + Send + Sync>
```

There is no `DaemonRuntimeObservability` extension trait and no ATM-owned
flush-at-shutdown contract. `atm-daemon-bootstrap` owns construction of the
concrete retained-log adapter at the Tokio/Axum composition boundary; the
adapter's bounded writer teardown remains an implementation detail of
`sc-observability`.

This keeps the only injected surface to the four core operations
(`emit`, `query`, `follow`, and `health`) and avoids a second daemon-specific
emission API alongside `ObservabilityPort`.

## Event Model

The historical `DaemonEvent` / `TeamScope` model is retired. The active
replacement daemon emits only the core `CommandEvent` model through
`ObservabilityPort`; its identifier fields are already validated by the core
contract. No daemon-specific event model or mapper remains in live code.

## Lifecycle Typestate Decision

Phase V.1 does not move daemon lifecycle control to a typestate API.

Decision:
- keep the runtime lifecycle as an explicit runtime-owned state machine
- keep legal transitions documented and enforced through the runtime boundary
- do not introduce `PhantomData` lifecycle state tokens in Phase V

Rationale:
- the lifecycle state spans process startup, listener publication, background
  worker drain, and cross-thread shutdown coordination
- the daemon already needs runtime-visible rollback and shutdown transitions
  that are easier to review as one explicit state machine
- the current `RuntimeLifecycleState` enum already expresses the legal daemon
  states directly

`LaunchGateGuard` alignment:
- `LaunchGateGuard` is a host admission and startup serialization primitive,
  not a daemon lifecycle typestate token
- it should remain a runtime coordination guard rather than becoming the
  carrier of `Starting` / `Serving` / `Stopping` type information

## Shared Observability Responsibilities

The shared daemon observability layer may own only:
- bootstrap
- sink setup
- emit
- query
- follow
- health
- bounded retained-writer teardown through the sink policy

It must not own:
- subsystem-specific event reconstruction
- daemon-wide wording policy for subsystem meaning
- message-context inference
- runtime coordination or control-plane behavior

## V.2 Migration Targets

Historical `V.2` migration targets included the now-retired AM.3 path
`crates/atm-daemon/src/local_ipc_transport.rs`; it is not a current subsystem.
The remaining historical targets were:
- `crates/atm-daemon/src/notification_runtime.rs`
- `crates/atm-daemon/src/watch_runtime.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/host_ownership.rs`
- `crates/atm-daemon/src/lifecycle_control.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`

Historical migration note:
- the retired `advisory_runtime.rs`, `notification_runtime.rs`,
  `watch_runtime.rs`, and `reconcile_runtime.rs` subsystem files remain listed
  in this document only as historical Phase V migration targets from the
  pre-`ADR-019` line

Shared wiring and test/support surfaces that must follow the final boundary are
all `V.2` refactor targets, with `V.3` shim cleanup where compatibility-only
paths remain:
- `crates/atm-daemon-bootstrap/src/daemon_observability.rs` (the source-visible
  implementation recorded by [`../../boundaries/atm-daemon-bootstrap/daemon-observability.toml`](../../boundaries/atm-daemon-bootstrap/daemon-observability.toml))
- `crates/atm-daemon-bootstrap/src/lib.rs`

First CI-checkable enforcement target for `V.2`:
- the shared observability layer must not import daemon subsystem types

## V.3 Deletion Targets

`V.3` must delete or collapse the old central mapping path in:
- `crates/atm-daemon-bootstrap/src/daemon_observability.rs`
  - `emit_runtime_event(...)`
  - `map_command_event(...)`
  - `map_runtime_event(...)`
  - centralized wording/field policy for subsystem-owned events
- any test-support shaping that exists only to preserve the central mapper
