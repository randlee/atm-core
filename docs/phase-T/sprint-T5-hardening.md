# Sprint T.5 Remaining Hardening

**Branch**: `integrate/phase-T`
**Base**: `integrate/phase-T @ bdac03c`
**PR target**: `develop`
**Status**: Planning

## Goal

Close the remaining daemon bounded-state and shutdown-contract gaps left open
after S.14 / S.15, including the production-review follow-up items that do not
belong in the SQLite or Windows-parity sprints.

## Deliverables

- fix `RuntimeStatusCache` insert-time overflow and all-conflict eviction
  failure so the documented bound is always enforced
- reconcile the daemon shutdown deadline contract between code and docs
- bound reconcile fingerprint retention beyond key-count-only caps
- harden remaining shutdown-lane follow-ups called out by the integration gate:
  - `NotificationRuntime::shutdown()` bounded join
  - pre-connect terminate checks in peer transport retries
  - orphaned shutdown-helper debug visibility
  - `SHUTDOWN_FINALIZER_THREADS` poison and rationale hardening
- keep the retained-log shutdown behavior and doctor/runtime projection aligned
  with the documented operator contract

## Key File Targets

- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-daemon/src/notification_runtime.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/lib.rs`
- `docs/architecture.md`
- `docs/atm-daemon/architecture.md`
- `docs/requirements.md`

## Acceptance Criteria

- the runtime status cache cannot exceed its documented bound, including
  conflict-heavy saturation cases
- reconcile fingerprint retention is bounded by an explicit implementation
  contract and regression tests
- code and docs agree on one shutdown deadline budget
- remaining open runtime/shutdown follow-ups from the production review and
  integration gate are closed or explicitly re-triaged with rationale

## QA Pointers

- `req-qa` must verify the shutdown budget in code and docs matches exactly
- service-hardening review should focus on bounded shutdown, poison handling,
  and termination latency
- `arch-qa` should confirm the fixes do not widen ownership or boundary scope

## Dependencies

- may run independently of `T.2` / `T.4`
- should be reviewed with the open `INTG-RSH-*` and production-review findings
  in hand rather than as a generic cleanup sprint
