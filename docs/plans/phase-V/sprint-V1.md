# Sprint V.1 — Observability Boundary And Event Model

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.1"
status: implemented
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pV-s1-observability-boundary
branch: feature/pV-s1-observability-boundary
estimated_scope: M
```

## Goal

Define the final daemon observability boundary so observability sits at the
bottom of the stack and subsystem semantics stay in the owning subsystem.

Carry-forward reference:
- `RULE-002` / `ARCH-PU-002` from `docs/plans/phase-U/sprint-U11.md` are the direct
  source findings for this sprint.

Dependency note:
- `V.1` is the prerequisite for `V.2` and `V.3`.
- `V.1` is a pure design sprint. The first CI-checkable enforcement for this
  observability line lands in `V.2`, which must prove the shared
  observability layer no longer requires daemon subsystem types.

## Scope

- define the injected daemon observability trait shape
- define the daemon subsystem event model
- make `subsystem` explicit on daemon events
- make `team` and message-context fields per-event payload rather than injected
  logger state
- define the hard boundary rule:
  observability depends on no daemon subsystem types
- update daemon requirements, architecture, and boundary docs to the final
  ownership model
- identify the concrete migration and deletion set in the current daemon line,
  including:
  - `crates/atm-daemon/src/daemon_runtime_observability.rs`
  - `crates/atm-daemon/src/daemon_observability.rs`
  - `crates/atm-daemon/src/runtime_health.rs`
  - `crates/atm-daemon/src/local_ipc_transport.rs`
  - `crates/atm-daemon/src/advisory_runtime.rs`
  - `crates/atm-daemon/src/notification_runtime.rs`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `crates/atm-daemon/src/watch_runtime.rs`
  - `crates/atm-daemon/src/reconcile_runtime.rs`
  - `crates/atm-daemon/src/host_ownership.rs`
  - `crates/atm-daemon/src/lifecycle_control.rs`
  - `crates/atm-daemon/src/runtime_status_cache.rs`
- commit the V.4 recovery text rule set to a persistent reference file so
  daemon/client runtime recovery guidance does not live only in sprint notes

## Acceptance Criteria

- the final daemon observability boundary is documented
- observability is explicitly bottom-of-stack
- daemon subsystems depend on the injected observability trait, never the
  reverse
- the observability trait is object-safe; `Arc<dyn DaemonRuntimeObservability>`
  is the final unrestricted consumption shape for the design
- the observability trait sealing decision is documented; it is either sealed
  by default or explicitly left open with rationale
- the daemon lifecycle typestate decision is documented; if lifecycle states do
  not use typestate, the rationale must say why `Starting` / `Serving` /
  `Stopping` remain runtime-only and how `LaunchGateGuard` aligns with that
  choice
- the event model documents the chosen typed representation for each semantic
  identifier and the rationale for that choice; `subsystem`, `message_id`, and
  `task_id` must each name the intended enum, newtype, or domain type and must
  not be left as raw `String` fields in the final design
- `team`, `agent`, `sender`, `recipient`, `message_id`, and `task_id` are
  documented as per-event fields when relevant
- the final subsystem/event model is clear enough to implement without central
  daemon event reconstruction
- the sprint doc or resulting architecture docs name the concrete code
  touchpoints that `V.2` and `V.3` must migrate or delete
- the sprint output makes it explicit that `ARCH-PU-002` / `RULE-002` stay
  in-scope and move forward into `V.2` / `V.3`
- the sprint output explicitly points `V.2` at the first CI-checkable
  enforcement point for this line: the shared observability layer must not
  require daemon subsystem types
- the daemon/client recovery text rule set is committed as a persistent
  reference file rather than living only in sprint notes

## Out Of Scope

- code migration into subsystems
- deleting old mapping code
- general runtime failure recovery work outside observability

## Sprint Output

- `docs/atm-daemon/observability.md`
- `docs/atm-daemon/recovery-text-rules.md`
- aligned updates in:
  - `docs/atm-daemon/requirements.md`
  - `docs/atm-daemon/architecture.md`
  - `docs/atm-daemon/boundaries.md`
  - `docs/architecture.md`

## Implementation Record

- `V.1` established `SubsystemObservability` as the daemon-side observability
  boundary shape that later sprints would inject into the owning runtime
  subsystems.
- `V.1` documented the scoping decision that `SubsystemObservability`
  injection is daemon-only work; `atm-daemon-client` stays outside that
  subsystem boundary and therefore does not consume the daemon-side injection
  surface.
- `V.1` fixed the shared wiring approach for `V.2` and `V.3`: subsystem-owned
  event construction moves into daemon runtime modules while
  `daemon_observability.rs` is reduced to thin sink/bootstrap responsibilities
  and cleanup/deletion follows in the later sprint.
