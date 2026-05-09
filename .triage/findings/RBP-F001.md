# RBP-F001: Missing rationale comment on env_lock / lock acquisition

## Pattern
```
env_lock\(\)
\.lock\(\)\.unwrap\(\)
\.lock\(\)\.expect\(
```

## Crates Affected
- atm-daemon

## Sprint Origin
R.14 (first reported R.14-QA-5)

## Status
open

## Description
Lock acquisitions (especially `env_lock()` and mutex `.lock()`) lack a rationale comment explaining WHY the lock is needed at that point. Rust best practices require non-obvious lock sites to document the invariant being protected. Silent `.unwrap()` on lock is also flagged — use `.expect("reason")` with context.

## Occurrences
| Branch | File | Line | Snippet | Fixed |
|--------|------|------|---------|-------|
| R.14 | crates/atm-daemon/src/tests.rs | TBD | `env_lock()` without rationale | open (abandoned branch) |
| R.15 | TBD | TBD | propagated | open |
| R.16 | TBD | TBD | propagated | open |
| R.17 | TBD | TBD | sweep needed | open |

## Fix
Add rationale comment immediately above each lock acquisition: `// Holds env_lock to prevent concurrent mutation of ATM_IDENTITY during test setup`. Use `.expect("context: why this lock cannot be poisoned")` not `.unwrap()`.

## Fix History
- 2026-05-07: First reported R.14-QA-5 [I]. Needs sweep on R.17.

## QA Round History
- R.14-QA-5: IMPORTANT
