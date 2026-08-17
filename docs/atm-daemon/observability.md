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

The injected daemon observability trait remains object-safe and sealed.

Final shape:

```rust
pub trait DaemonRuntimeObservability:
    atm_core::boundary::sealed::Sealed + ObservabilityPort + Send + Sync
{
    fn emit_daemon_event(&self, event: DaemonEvent) -> Result<(), AtmError>;

    fn best_effort_flush_blocking(&self) -> Result<(), AtmError>;
}
```

Consumption shape:

```rust
Arc<dyn DaemonRuntimeObservability>
```

Object-safety rationale:
- no generic methods
- no `Self` return position
- no by-value `self` receiver requirements
- the trait remains suitable for one injected trait object shared across daemon
  runtime partitions

Sealing decision:
- sealed by default
- rationale:
  - the daemon does not currently need third-party implementers
  - Phase V is closing an internal architecture seam, not creating a public
    plugin surface
  - sealing allows the daemon observability contract to evolve through `V.2`
    and `V.3` without external implementation commitments

## Event Model

The daemon event model is subsystem-oriented, not central-mapper-oriented.

Final shape:

```rust
pub struct DaemonEvent {
    pub subsystem: DaemonSubsystem,
    pub action: &'static str,
    pub outcome: &'static str,
    pub team: TeamScope,
    pub agent: Option<AgentName>,
    pub sender: Option<AgentName>,
    pub recipient: Option<AgentName>,
    pub message_id: Option<AtmMessageId>,
    pub task_id: Option<TaskId>,
    pub detail: Cow<'static, str>,
}
```

Typed identifier decisions:
- `subsystem`: `DaemonSubsystem` enum
  - rationale:
    - the daemon subsystem set is finite and reviewable
    - enum closure prevents string drift and ad hoc subsystem naming
- `message_id`: `AtmMessageId`
  - rationale:
    - ATM already owns a validated message-id newtype
    - the daemon must not fall back to raw string message identifiers
- `task_id`: `TaskId`
  - rationale:
    - ATM already owns a validated task-id domain type
    - retaining the newtype keeps task identifiers consistent across CLI,
      daemon, and schema boundaries

Recommended live daemon subsystem enum members:
- `Bootstrap`
- `Composition`
- `LocalIpcTransport`
- `RuntimeHealth`
- `HostOwnership`
- `LifecycleControl`
- `RuntimeStatusCache`
- `ObservabilitySink`

Historical-only subsystem names retained in this record:
- `AdvisoryRuntime`
- `NotificationRuntime`
- `WatchRuntime`
- `ReconcileRuntime`

Historical-runtime note:
- the subsystem names above are retained in this Phase V.1 observability
  record only for historical migration bookkeeping from the earlier
  compatibility line
- they are not accepted live runtime lanes after `ADR-019`

## Per-Event Context Fields

`team`, `agent`, `sender`, `recipient`, `message_id`, and `task_id` are
per-event payload fields, not injected logger state.

Chosen representation for team scope:

```rust
pub enum TeamScope {
    Team(TeamName),
    None,
}
```

Rationale:
- most daemon events are team-scoped
- infrastructure events such as daemon startup and shutdown are not
- explicit `TeamScope::None` is clearer than inferring absence from logger
  construction or from unrelated sender/recipient state

Field rules:
- `team` is required on each event as `TeamScope`
- `agent`, `sender`, and `recipient` are optional and event-specific
- `message_id` and `task_id` are optional and event-specific
- no daemon subsystem may rely on hidden ambient logger state to fill these
  values later

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
- best-effort synchronous flush during shutdown

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
- `crates/atm-daemon/src/daemon_runtime_observability.rs`
  - `V.2` required action: refactor this file to the final injected trait and
    daemon event surface used by subsystem-owned emission
  - `V.3` follow-through: delete any compatibility shims that exist only to
    preserve the old central mapper path
- `crates/atm-daemon/src/daemon_observability.rs`
- `crates/atm-daemon/src/composition.rs` (retired by AM.3)
- `crates/atm-daemon/src/main.rs`
- `crates/atm-daemon/src/lib.rs`
- `crates/atm-daemon/src/test_observability.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/test_support.rs`
- `crates/atm-daemon/src/tests.rs`
- `crates/atm-daemon/src/tests_post_send_graft_warning.rs`
- `crates/atm-daemon/src/tests_lifecycle.rs`

First CI-checkable enforcement target for `V.2`:
- the shared observability layer must not import daemon subsystem types

## V.3 Deletion Targets

`V.3` must delete or collapse the old central mapping path in:
- `crates/atm-daemon/src/daemon_observability.rs`
  - `emit_runtime_event(...)`
  - `map_command_event(...)`
  - `map_runtime_event(...)`
  - centralized wording/field policy for subsystem-owned events
- any test-support shaping that exists only to preserve the central mapper
