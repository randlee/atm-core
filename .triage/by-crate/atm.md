# atm — Active Findings Index

Crate path: `crates/atm/`

## Open Findings

| ID | Severity | File | Summary | Status |
|----|----------|------|---------|--------|
| CI-WIN-001 | BLOCKING | src/composition.rs:5,33-34,628-629 | Ungated unix-only Duration import + constants + test imports | fixed (e91f97f) |

## Fixed Findings

| ID | File | Fix Commit | Sprint |
|----|------|-----------|--------|
| CI-WIN-001 | src/composition.rs | e91f97f | R.13-CI-FIX-R2 |

## Notes
- e91f97f added non-Unix dead-code gates for Duration, SAME_HOST_REQUEST_DEADLINE, AUTO_START_PUBLISH_TIMEOUT, DaemonSupervisor, LocalSocketClientTransport
- Propagation to R.15/R.16/R.17 via merge-forward needed — sweep required
