# Sprint V.3 — Observability Removal And Streamlining

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.3"
status: planned
worktree: TBD
branch: TBD
estimated_scope: M
```

## Goal

Delete the old central daemon observability mapping shape and streamline the
final implementation so the system is ready for testing.

## Scope

- reduce `crates/atm-daemon/src/daemon_observability.rs` to thin sink and
  bootstrap responsibilities
- remove central daemon event reconstruction helpers and wrappers
- consolidate any now-redundant event-shaping paths left over from the old
  model
- make the final implementation lean without losing useful runtime visibility

## Acceptance Criteria

- `crates/atm-daemon/src/daemon_observability.rs` is thin and bottom-of-stack
- obsolete mapping code introduced by the old central model is removed
- duplicate event-shaping paths are consolidated
- the resulting code shape is lean enough to support system testing without
  carrying transitional observability baggage

## Out Of Scope

- defining the event model from scratch
- general daemon cleanup outside observability
- broader runtime recovery work
