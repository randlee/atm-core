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

## Scope

- inject the thin observability trait into daemon subsystems
- move subsystem event construction into the owning subsystem
- keep shared observability responsibilities limited to:
  bootstrap, sink setup, emit, query, follow, and health
- preserve structured event coverage needed for system testing
- add any required enforcement note for the bottom-of-stack dependency
  direction

## Acceptance Criteria

- daemon subsystems emit their own structured events through the injected trait
- shared observability infrastructure no longer reconstructs subsystem meaning
- the daemon observability path remains suitable for system testing and runtime
  diagnosis
- no daemon subsystem type is required by the shared observability layer

## Out Of Scope

- final deletion of all old wrappers and mapping helpers
- unrelated daemon runtime cleanup
- broader recovery hardening outside observability
