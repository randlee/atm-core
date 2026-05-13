# Sprint V.5 — Daemon Observability Boundary Cleanup

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.5"
status: planned
worktree: TBD
branch: TBD
estimated_scope: M
```

## Goal

Close `ARCH-PU-002` by making daemon observability a thin bottom-of-stack sink
boundary with subsystem-owned event semantics.

Carry-forward reference:
- `docs/phase-U/sprint-U11.md` records the `ARCH-PU-002` deferral note that
  this sprint is required to close.
- `RULE-002` is the concrete carried-forward violation:
  `crates/atm-daemon/src/daemon_observability.rs` `emit_runtime_event`
  centralizes daemon event reconstruction in the bottom-of-stack observability
  layer.

Dependency note:
- the observability ownership redesign is independent of `V.1` runtime test
  isolation and `V.2` workspace `#[path]` enforcement at the design level, but
  `V.5` should build on the lint framework they establish when adding any hard
  boundary rule that prevents observability from importing subsystem-owned
  types.

## Scope

- address the carry-forward issue in
  `crates/atm-daemon/src/daemon_observability.rs`
- replace central daemon event reconstruction with subsystem-owned event
  emission through an injected thin logging trait
- define explicit daemon event shape rules:
  - every daemon event carries `subsystem`
  - `team` scope is per-event and may be explicit none/sentinel for daemon
    infrastructure events
  - `agent`, `sender`, `recipient`, `message_id`, and `task_id` are optional
    per-event context fields when relevant
- keep shared observability responsibilities thin:
  bootstrap, sink setup, emit, query, follow, and health only
- add a hard boundary rule or lint preventing observability from importing
  subsystem-owned types
- remove or streamline obsolete mapping helpers and wrappers after the new
  ownership model lands

## Acceptance Criteria

- `ARCH-PU-002` is closed rather than carried forward again
- daemon observability depends on no daemon subsystem types
- subsystems emit their own structured events through the injected logging
  boundary; observability does not reconstruct subsystem semantics centrally
- event context rules are explicit:
  `team` and message metadata are per-event payload, not injected logger state
- `crates/atm-daemon/src/daemon_observability.rs` is reduced to thin sink and
  bootstrap responsibilities
- obsolete mapping code introduced by the old central model is removed or
  consolidated
- the final boundary is documented in daemon requirements, architecture, and
  boundary docs

## Out Of Scope

- changing the shared `sc-observability` external contract
- redesigning CLI observability ownership outside the daemon cleanup line
- adding new observability backends or product features
