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
- obtain and record the authoritative shutdown deadline ruling for daemon
  graceful drain and force-cancel budgets; if the ruling is still unresolved,
  keep it as a named blocking gap in this sprint rather than silently choosing
  values
- bound reconcile fingerprint retention beyond key-count-only caps
  by naming whether the bound is:
  - per-key
  - global
  - or both
  and by naming the enforcement form (for example, explicit eviction or a
  bounded map/set contract)
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

- the runtime status cache cannot exceed the documented `4096`-entry cap,
  including a regression test that saturates the cache at exactly `4096`
  entries with all-conflict records
- reconcile fingerprint retention is bounded by an explicit implementation
  contract and regression tests
- `REQ-P-DAEMON-LANES-001` is satisfied by an explicit shutdown budget contract;
  until the 2s/3s vs 5s/10s ruling is made, `GAP-T5-001` remains open and T.5
  cannot claim closeout on that item
- remaining open runtime/shutdown follow-ups from the production review and
  integration gate are closed or explicitly re-triaged with rationale

## Named Gaps

- `GAP-T5-001` — authoritative shutdown values unresolved:
  - code currently uses `2s` / `3s`
  - architecture docs currently state `5s` / `10s`
  - T.5 must not invent values; it must either record the accepted ruling or
    remain open on this item

## QA Pointers

- `req-qa` must verify the shutdown budget in code and docs matches exactly
- service-hardening review should focus on bounded shutdown, poison handling,
  and termination latency
- `arch-qa` should confirm the fixes do not widen ownership or boundary scope

## Dependencies

- may run independently of `T.2` / `T.4`
- should be reviewed with the open `INTG-RSH-*` and production-review findings
  in hand rather than as a generic cleanup sprint
