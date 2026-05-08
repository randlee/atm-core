# ADR-006 — Bounded SIGHUP Reload Delivery In R.18

| Field | Value |
|---|---|
| ID | ADR-006 |
| Status | **Accepted For Implementation** |
| Date | 2026-05-08 |
| Deciders | Rand Lee |
| Relates to | REQ-P-RUNTIME-001, REQ-P-RUNTIME-002, ADR-002 |
| Supersedes | Informal sprint-only SIGHUP deferral note |

---

## Context

Phase R delivers the daemon runtime shape, singleton ownership, host-scoped
SQLite state, peer replay, runtime health, and the watch/reconcile lanes.

The final runtime contract also requires bounded `SIGHUP` handling so the
daemon can reload config and roster state without dropping singleton
ownership, while preserving a last-known-good serving view on reload failure.

Phase-end review identified that this reload contract must be delivered as one
bounded operation rather than as a partial late-phase patch. `R.18` is the
delivery sprint for that implementation.

## Decision Drivers

- singleton/runtime safety is more important than partial reload support
- config reload must preserve a last-known-good serving view
- reload validation and swap semantics must be explicit and testable
- operators need a bounded reload path instead of restart-only operations

## Options Considered

### Option 1 — Keep Ignoring `SIGHUP` Until A Later Phase

Continue serving the current runtime view and leave reload unimplemented.

**Rejected.** The architecture already requires bounded reload, and `R.18` is
the committed delivery sprint for closing that gap.

### Option 2 — Deliver Bounded Reload In `R.18`

Implement one bounded reload path with validation, last-known-good
preservation, and typed failure behavior.

**Accepted.**

## Decision

`R.18` delivers bounded `SIGHUP` config/roster reload.

Required runtime behavior:

- `SIGHUP` triggers a bounded reload attempt
- candidate config and roster input validate before replacing the active
  serving view
- invalid reload input yields a typed reload failure and preserves the
  last-known-good serving configuration
- singleton ownership remains held throughout reload attempt and rollback
- reload success and failure paths are covered by dedicated regression tests

## Consequences

### Positive

- the runtime matches the documented bounded reload contract
- singleton/runtime continuity remains explicit and testable
- operators gain a reload path that preserves last-known-good serving state

### Negative

- `R.18` must carry reload implementation, validation, and test coverage as a
  single sprint deliverable
- late changes to reload shape after `R.18` implementation would risk
  divergence from the architecture contract

## Follow-Up Work

| Action | Owner | Gate |
|---|---|---|
| Implement bounded config/roster reload | `arch-ctm` | `R.18` acceptance |
| Add reload regression tests | `arch-ctm` | `R.18` validation |
| Replace the temporary log-only `SIGHUP` path with the bounded reload path | `arch-ctm` | `R.18` merge |
