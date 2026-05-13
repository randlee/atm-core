# Sprint V.2 — Observability Migration Into Subsystems

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.2"
status: planned
worktree: TBD
branch: TBD
estimated_scope: M
```

## Goal

Move daemon observability semantics into the owning subsystems while keeping
shared observability infrastructure thin.

Carry-forward reference:
- `RULE-002` / `ARCH-PU-002` continue here as the migration step after `V.1`;
  this sprint is where subsystem-owned event semantics replace the central
  daemon mapping path in practice.

Dependency note:
- `V.2` depends on `V.1`.

## Scope

- inject the thin observability trait into daemon subsystems
- move subsystem event construction into the owning subsystem
- keep shared observability responsibilities limited to:
  bootstrap, sink setup, emit, query, follow, and health
- preserve structured event coverage needed for system testing
- add any required enforcement note for the bottom-of-stack dependency
  direction
- itemize the current subsystem migration set explicitly:
  - `crates/atm-daemon/src/local_ipc_transport.rs`
  - `crates/atm-daemon/src/advisory_runtime.rs`
  - `crates/atm-daemon/src/notification_runtime.rs`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `crates/atm-daemon/src/watch_runtime.rs`
  - `crates/atm-daemon/src/reconcile_runtime.rs`
  - `crates/atm-daemon/src/runtime_health.rs`
  - `crates/atm-daemon/src/host_ownership.rs`
  - `crates/atm-daemon/src/lifecycle_control.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
- update the shared composition and test-support surfaces that wire or exercise
  daemon observability:
  - `crates/atm-daemon/src/daemon_runtime_observability.rs`
  - `crates/atm-daemon/src/daemon_observability.rs`
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm-daemon/src/main.rs`
  - `crates/atm-daemon/src/lib.rs`
  - `crates/atm-daemon/src/test_observability.rs`
  - `crates/atm-daemon/src/runtime_health_test_support.rs`
  - `crates/atm-daemon/src/test_support.rs`
  - `crates/atm-daemon/src/tests.rs`
  - `crates/atm-daemon/src/tests_advisory.rs`
  - `crates/atm-daemon/src/tests_lifecycle.rs`
- reduce `crates/atm-daemon/src/daemon_observability.rs` during migration so it
  stops owning subsystem semantics, while leaving final wrapper/helper deletion
  to `V.3`

## Acceptance Criteria

- daemon subsystems emit their own structured events through the injected trait
- shared observability infrastructure no longer reconstructs subsystem meaning
- the daemon observability path remains suitable for system testing and runtime
  diagnosis
- no daemon subsystem type is required by the shared observability layer
- the sprint plan identifies every subsystem file where logging ownership must
  move so the migration does not stop at `runtime_health`
- `ARCH-PU-002` / `RULE-002` are advanced materially by moving event ownership
  out of the central daemon layer
- `crates/atm-daemon/src/daemon_observability.rs` is explicitly accounted for
  as an in-scope reduction target in preparation for `V.3` removal/streamlining

## Out Of Scope

- final deletion of all old wrappers and mapping helpers
- unrelated daemon runtime cleanup
- broader recovery hardening outside observability
