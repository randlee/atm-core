# Sprint V.3 — Observability Removal And Streamlining

```yaml
plan_type: sprint_plan
phase: V
sprint: "V.3"
status: implemented
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pV-s3-observability-cleanup
branch: feature/pV-s3-observability-cleanup
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
  - `map_daemon_event(...)`
  - `map_runtime_event(...)`
  - `record_daemon_event(...)`
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

## Implementation Record

### AC5 Deletion Record

Confirmed removed from the final `V.3` code path:
- `map_command_event(...)`
- `map_daemon_event(...)`
- `map_runtime_event(...)`
- `emit_runtime_event(...)`
- `record_daemon_event(...)`

Callsite migration record:
- command-side ATM observability emission now stays on
  `ObservabilityPort::emit(...)` in
  `crates/atm-daemon/src/daemon_observability.rs`
- daemon subsystem emission now routes through
  `SubsystemObservability::emit(...)` /
  `SubsystemObservability::emit_event(...)` in
  `crates/atm-daemon/src/daemon_runtime_observability.rs`
- the injected daemon sink trait now receives fully shaped subsystem events via
  `DaemonRuntimeObservability::emit_daemon_event(...)`
- shared observability sink shaping remains only in
  `DaemonObservability::emit_log_event(...)` as bottom-of-stack retained-log
  output, not as a daemon-wide semantic reconstruction layer

Deletion-vs-rewrite summary:
- deleted:
  - old central mapper helpers listed above
  - the central daemon event reconstruction path they supported
- rewritten:
  - daemon subsystem callsites to emit already-shaped `DaemonEvent` payloads
    through injected subsystem observability handles
  - retained sink emission to operate on final shaped payloads instead of
    subsystem-specific reconstruction helpers

## Out Of Scope

- defining the event model from scratch
- general daemon cleanup outside observability
- broader runtime recovery work
