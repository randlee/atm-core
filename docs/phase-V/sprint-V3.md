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

Carry-forward reference:
- `RULE-002` / `ARCH-PU-002` are closed here by deleting the old central
  daemon observability reconstruction path after `V.2` lands.

Dependency note:
- `V.3` depends on `V.2`.

## Scope

- reduce `crates/atm-daemon/src/daemon_observability.rs` to thin sink and
  bootstrap responsibilities
- remove central daemon event reconstruction helpers and wrappers, specifically
  reviewing:
  - `emit_runtime_event(...)`
  - `map_command_event(...)`
  - `map_runtime_event(...)`
  - any remaining daemon-wide wording/field-policy helper that exists only to
    rebuild subsystem semantics centrally
- consolidate any now-redundant event-shaping paths left over from the old
  model
- make the final implementation lean without losing useful runtime visibility
- remove or collapse any no-longer-needed observability test/support seams that
  existed only for the old central mapping model:
  - `crates/atm-daemon/src/test_observability.rs`
  - `crates/atm-daemon/src/runtime_health_test_support.rs`
  - `crates/atm-daemon/src/test_support.rs`

## Acceptance Criteria

- `crates/atm-daemon/src/daemon_observability.rs` is thin and bottom-of-stack
- obsolete mapping code introduced by the old central model is removed
- duplicate event-shaping paths are consolidated
- the resulting code shape is lean enough to support system testing without
  carrying transitional observability baggage
- the sprint output clearly identifies what code was deleted versus merely
  rewritten
- the sprint closes the remaining `ARCH-PU-002` / `RULE-002` implementation
  debt rather than carrying it forward again

## Out Of Scope

- defining the event model from scratch
- general daemon cleanup outside observability
- broader runtime recovery work
