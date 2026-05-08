# ADR-006 — Bounded SIGHUP Reload Deferred To R.18

| Field | Value |
|---|---|
| ID | ADR-006 |
| Status | **Accepted** |
| Date | 2026-05-08 |
| Deciders | Rand Lee |
| Relates to | REQ-P-RUNTIME-001, REQ-P-RUNTIME-002, ADR-002 |
| Supersedes | Informal sprint-only SIGHUP deferral note |

---

## Context

Phase R delivers the daemon runtime shape, singleton ownership, host-scoped
SQLite state, peer replay, runtime health, and the watch/reconcile lanes.

The documented daemon runtime also calls for bounded `SIGHUP` handling so the
daemon can reload config and roster state without dropping singleton ownership.
That reload path was planned but not completed within the Phase R closeout
scope.

The runtime currently has a safe fallback:

- continue serving with the last-known-good runtime view
- ignore `SIGHUP` rather than attempting a partial or unsafe reload

The question is whether Phase R should ship an incomplete reload path or defer
the feature until it can be implemented as one coherent bounded operation.

## Decision Drivers

- singleton/runtime safety is more important than partial reload support
- config reload must preserve a last-known-good serving view
- partial reload logic would be hard to validate late in the phase
- the daemon already has a safe operational fallback: ignore `SIGHUP`

## Options Considered

### Option 1 — Implement Partial Reload In Phase R

Add a limited reload path before the phase-end merge.

**Rejected.** This creates late-phase risk in singleton ownership, runtime
continuity, and config consistency.

### Option 2 — Defer Bounded Reload To R.18 And Ignore SIGHUP Until Then

Keep the daemon on its current last-known-good serving view and make the
deferral explicit.

**Accepted.**

## Decision

Bounded `SIGHUP` config/roster reload is deferred from the Phase R merge line
to `R.18`.

Phase R daemon behavior:

- `SIGHUP` is observed
- the daemon does not reload config or roster state
- the daemon continues serving the current last-known-good runtime view
- the deferral is logged and documented rather than silently ignored

`R.18` owns the implementation of:

- bounded reload execution
- candidate validation before swap
- last-known-good preservation on reload failure
- regression tests for reload during steady-state serving

## Consequences

### Positive

- no unsafe partial reload path ships in Phase R
- singleton/runtime continuity remains simple and testable
- the runtime keeps serving from a known-good configuration

### Negative

- operators cannot refresh config or roster state with `SIGHUP` during Phase R
- config changes still require the documented restart path until `R.18` lands

## Follow-Up Work

| Action | Owner | Gate |
|---|---|---|
| Implement bounded config/roster reload | `arch-ctm` | `R.18` acceptance |
| Add reload regression tests | `arch-ctm` | `R.18` validation |
| Enforce the per-connection `32` in-flight request cap (`crates/atm-daemon/src/lib.rs:648`) | `arch-ctm` | `R.18` acceptance |
| Replace the deferral log-only path with the real reload path | `arch-ctm` | `R.18` merge |
