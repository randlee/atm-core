# atm-daemon — Active Findings Index

Crate path: `crates/atm-daemon/`

## Open Findings

| ID | Severity | File | Summary | Status |
|----|----------|------|---------|--------|
| RULE-005 | BLOCKING | retired peer transport | Duplicate replay DTO | closed (peer transport deleted) |
| FTQ-001 | BLOCKING | src/tests.rs:28-29 | OnceLock global dispatcher — parallel test race | open |
| FTQ-002 | IMPORTANT | src/tests.rs:136 | Fixed 50ms sleep — timing flaky on slow CI | open |
| FTQ-005 | IMPORTANT | src/tests.rs:148 | singleton_guard_recovers_stale_owner shared-state race | open |
| RBP-F001 | IMPORTANT | src/tests.rs | env_lock() missing rationale comment | open |
| RBP-F002 | IMPORTANT | src/runtime_health.rs:268 | mark_sqlite_unavailable silent lock-poison swallow | closed (tracing::error! added R.15-FIX-R3) |
| CI-WIN-001 | BLOCKING | src/composition.rs | Ungated unix-only imports/symbols | fixed-partial |

## Fixed Findings

| ID | File | Fix Commit | Sprint |
|----|------|-----------|--------|
| CI-WIN-001 | src/composition.rs:7 | b36c946 | R.13-FIX-R5 |

## Notes
- FTQ-001 is highest priority — blocks parallel test execution across R.15/R.16/R.17
- RBP-F002 in runtime_health.rs:268 likely addressed in R.15-FIX-R3
